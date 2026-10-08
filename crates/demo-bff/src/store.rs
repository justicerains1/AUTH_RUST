//! Persistent BFF authority. Namespace is included in every lookup and AEAD purpose.
use identity_core::{
    clock::Clock,
    security::{AeadEnvelope, AeadKeyRing, token_digest},
};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::{fmt, sync::Arc};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    InvalidNamespace,
    InvalidFlow,
    InvalidSession,
    InvalidTokens,
    Unavailable,
}
impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidNamespace => "invalid BFF namespace",
            Self::InvalidFlow => "invalid BFF login flow",
            Self::InvalidSession => "invalid BFF session",
            Self::InvalidTokens => "invalid BFF token state",
            Self::Unavailable => "BFF storage unavailable",
        })
    }
}
impl std::error::Error for StoreError {}
impl From<sqlx::Error> for StoreError {
    fn from(_: sqlx::Error) -> Self {
        Self::Unavailable
    }
}
pub struct LoginFlowInput {
    pub flow_cookie_hash: [u8; 32],
    pub state_hash: [u8; 32],
    pub nonce: Zeroizing<String>,
    pub pkce_verifier: Zeroizing<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FlowPayload {
    nonce: String,
    pkce_verifier: String,
}
impl Drop for FlowPayload {
    fn drop(&mut self) {
        self.nonce.zeroize();
        self.pkce_verifier.zeroize();
    }
}
pub struct LoginFlowSecrets {
    pub nonce: Zeroizing<String>,
    pub pkce_verifier: Zeroizing<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BffTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: String,
    pub access_expires_at: OffsetDateTime,
    pub session_expires_at: OffsetDateTime,
}
impl Drop for BffTokens {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
        self.id_token.zeroize();
    }
}
impl fmt::Debug for BffTokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BffTokens([REDACTED])")
    }
}
pub struct NewBffSession {
    pub cookie_hash: [u8; 32],
    pub user_id: Uuid,
    pub tokens: BffTokens,
    pub old_cookie_hash: Option<[u8; 32]>,
}
#[derive(Clone)]
pub struct BffStore {
    pool: PgPool,
    namespace: String,
    keys: Arc<AeadKeyRing>,
    clock: Arc<dyn Clock>,
}
impl BffStore {
    pub fn new(
        pool: PgPool,
        namespace: String,
        keys: Arc<AeadKeyRing>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, StoreError> {
        if namespace.is_empty()
            || namespace.len() > 64
            || !namespace.as_bytes()[0].is_ascii_lowercase()
            || !namespace
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b))
        {
            return Err(StoreError::InvalidNamespace);
        }
        Ok(Self {
            pool,
            namespace,
            keys,
            clock,
        })
    }
    pub async fn begin_login(&self, input: &LoginFlowInput) -> Result<Uuid, StoreError> {
        if !identity_core::oauth::valid_state(&input.nonce)
            || identity_core::oauth::pkce_s256(&input.pkce_verifier).is_err()
        {
            return Err(StoreError::InvalidFlow);
        }
        let now = self.clock.now();
        let id = Uuid::new_v4();
        let payload = FlowPayload {
            nonce: input.nonce.to_string(),
            pkce_verifier: input.pkce_verifier.to_string(),
        };
        let raw =
            Zeroizing::new(serde_json::to_vec(&payload).map_err(|_| StoreError::InvalidFlow)?);
        let envelope = self
            .keys
            .encrypt(id, &self.purpose("flow"), &raw)
            .map_err(|_| StoreError::Unavailable)?;
        sqlx::query("INSERT INTO bff_login_flows(id,namespace,cookie_hash,state_hash,encrypted_state,created_at,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(id).bind(&self.namespace).bind(input.flow_cookie_hash.as_slice()).bind(input.state_hash.as_slice()).bind(sqlx::types::Json(envelope)).bind(now).bind(now+Duration::minutes(5)).execute(&self.pool).await?;
        Ok(id)
    }
    pub async fn consume_login(
        &self,
        cookie: [u8; 32],
        state: [u8; 32],
    ) -> Result<LoginFlowSecrets, StoreError> {
        let mut tx = self.pool.begin().await?;
        let row=sqlx::query("SELECT id,encrypted_state,expires_at,consumed_at FROM bff_login_flows WHERE namespace=$1 AND cookie_hash=$2 AND state_hash=$3 FOR UPDATE").bind(&self.namespace).bind(cookie.as_slice()).bind(state.as_slice()).fetch_optional(&mut *tx).await?.ok_or(StoreError::InvalidFlow)?;
        let now = self.clock.now();
        if row
            .try_get::<Option<OffsetDateTime>, _>("consumed_at")?
            .is_some()
            || row.try_get::<OffsetDateTime, _>("expires_at")? <= now
        {
            return Err(StoreError::InvalidFlow);
        }
        let id: Uuid = row.try_get("id")?;
        let envelope: AeadEnvelope = serde_json::from_value(row.try_get("encrypted_state")?)
            .map_err(|_| StoreError::InvalidFlow)?;
        let raw = self
            .keys
            .decrypt(id, &self.purpose("flow"), &envelope)
            .map_err(|_| StoreError::InvalidFlow)?;
        let payload: FlowPayload =
            serde_json::from_slice(&raw).map_err(|_| StoreError::InvalidFlow)?;
        sqlx::query("UPDATE bff_login_flows SET consumed_at=$2 WHERE id=$1")
            .bind(id)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(LoginFlowSecrets {
            nonce: Zeroizing::new(payload.nonce.clone()),
            pkce_verifier: Zeroizing::new(payload.pkce_verifier.clone()),
        })
    }
    pub async fn create_session(&self, input: &NewBffSession) -> Result<Uuid, StoreError> {
        let now = self.clock.now();
        validate_tokens(&input.tokens, now, None)?;
        let id = Uuid::new_v4();
        let envelope = self.encrypt_tokens(id, &input.tokens)?;
        let mut tx = self.pool.begin().await?;
        if let Some(old) = input.old_cookie_hash {
            sqlx::query("UPDATE bff_sessions SET revoked_at=COALESCE(revoked_at,$3) WHERE namespace=$1 AND cookie_hash=$2").bind(&self.namespace).bind(old.as_slice()).bind(now).execute(&mut *tx).await?;
        }
        sqlx::query("INSERT INTO bff_sessions(id,namespace,cookie_hash,user_id,encrypted_tokens,created_at,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(id).bind(&self.namespace).bind(input.cookie_hash.as_slice()).bind(input.user_id).bind(sqlx::types::Json(envelope)).bind(now).bind(input.tokens.session_expires_at).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(id)
    }
    pub async fn lock_session(&self, cookie: [u8; 32]) -> Result<LockedSession<'_>, StoreError> {
        let mut tx = self.pool.begin().await?;
        let row=sqlx::query("SELECT id,user_id,encrypted_tokens,expires_at FROM bff_sessions WHERE namespace=$1 AND cookie_hash=$2 AND revoked_at IS NULL ORDER BY id FOR UPDATE").bind(&self.namespace).bind(cookie.as_slice()).fetch_optional(&mut *tx).await?.ok_or(StoreError::InvalidSession)?;
        let now = self.clock.now();
        let expiry: OffsetDateTime = row.try_get("expires_at")?;
        if expiry <= now {
            return Err(StoreError::InvalidSession);
        }
        let id: Uuid = row.try_get("id")?;
        let envelope: AeadEnvelope = serde_json::from_value(row.try_get("encrypted_tokens")?)
            .map_err(|_| StoreError::InvalidTokens)?;
        let raw = self
            .keys
            .decrypt(id, &self.purpose("tokens"), &envelope)
            .map_err(|_| StoreError::InvalidTokens)?;
        let tokens: BffTokens =
            serde_json::from_slice(&raw).map_err(|_| StoreError::InvalidTokens)?;
        if tokens.session_expires_at != expiry
            || tokens.access_expires_at > expiry
            || tokens.access_token.is_empty()
            || tokens.refresh_token.is_empty()
            || tokens.id_token.is_empty()
        {
            return Err(StoreError::InvalidTokens);
        }
        Ok(LockedSession {
            tx,
            id,
            user_id: row.try_get("user_id")?,
            tokens,
            namespace: &self.namespace,
            keys: &self.keys,
            clock: &self.clock,
            absolute_expiry: expiry,
        })
    }
    pub async fn delete_session(&self, cookie: [u8; 32]) -> Result<(), StoreError> {
        sqlx::query("UPDATE bff_sessions SET revoked_at=COALESCE(revoked_at,$3) WHERE namespace=$1 AND cookie_hash=$2").bind(&self.namespace).bind(cookie.as_slice()).bind(self.clock.now()).execute(&self.pool).await?;
        Ok(())
    }
    fn purpose(&self, kind: &str) -> String {
        format!("bff:{}:{kind}", self.namespace)
    }
    fn encrypt_tokens(&self, id: Uuid, tokens: &BffTokens) -> Result<AeadEnvelope, StoreError> {
        let raw =
            Zeroizing::new(serde_json::to_vec(tokens).map_err(|_| StoreError::InvalidTokens)?);
        self.keys
            .encrypt(id, &self.purpose("tokens"), &raw)
            .map_err(|_| StoreError::Unavailable)
    }
}
pub struct LockedSession<'a> {
    tx: Transaction<'a, Postgres>,
    pub id: Uuid,
    pub user_id: Uuid,
    tokens: BffTokens,
    namespace: &'a str,
    keys: &'a AeadKeyRing,
    clock: &'a Arc<dyn Clock>,
    absolute_expiry: OffsetDateTime,
}
impl LockedSession<'_> {
    pub fn tokens(&self) -> &BffTokens {
        &self.tokens
    }
    pub fn needs_refresh(&self) -> bool {
        self.tokens.access_expires_at <= self.clock.now() + Duration::seconds(15)
    }
    pub async fn release(self) -> Result<(), StoreError> {
        self.tx.commit().await.map_err(Into::into)
    }
    pub async fn replace_tokens(mut self, tokens: &BffTokens) -> Result<(), StoreError> {
        let now = self.clock.now();
        if let Err(error) = validate_tokens(tokens, now, Some(self.absolute_expiry)) {
            self.invalidate().await?;
            return Err(error);
        }
        let raw =
            Zeroizing::new(serde_json::to_vec(tokens).map_err(|_| StoreError::InvalidTokens)?);
        let envelope = self
            .keys
            .encrypt(self.id, &format!("bff:{}:tokens", self.namespace), &raw)
            .map_err(|_| StoreError::Unavailable)?;
        let result=sqlx::query("UPDATE bff_sessions SET encrypted_tokens=$2 WHERE id=$1 AND revoked_at IS NULL AND expires_at>$3").bind(self.id).bind(sqlx::types::Json(envelope)).bind(now).execute(&mut *self.tx).await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::InvalidSession);
        }
        self.tx.commit().await?;
        Ok(())
    }
    pub async fn invalidate(mut self) -> Result<(), StoreError> {
        sqlx::query("UPDATE bff_sessions SET revoked_at=COALESCE(revoked_at,$2) WHERE id=$1")
            .bind(self.id)
            .bind(self.clock.now())
            .execute(&mut *self.tx)
            .await?;
        self.tx.commit().await?;
        Ok(())
    }
}
fn validate_tokens(
    tokens: &BffTokens,
    now: OffsetDateTime,
    absolute: Option<OffsetDateTime>,
) -> Result<(), StoreError> {
    if tokens.access_token.is_empty()
        || tokens.refresh_token.is_empty()
        || tokens.id_token.is_empty()
        || tokens.access_expires_at <= now
        || tokens.access_expires_at > tokens.session_expires_at
        || tokens.session_expires_at <= now
        || tokens.session_expires_at > now + Duration::hours(12)
        || absolute.is_some_and(|expiry| tokens.session_expires_at > expiry)
    {
        return Err(StoreError::InvalidTokens);
    }
    Ok(())
}
pub fn cookie_digest(value: &str) -> [u8; 32] {
    token_digest(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use identity_core::clock::FixedClock;
    #[tokio::test]
    async fn namespace_and_token_boundaries_are_explicit_without_database_calls()
    -> Result<(), Box<dyn std::error::Error>> {
        let now = OffsetDateTime::UNIX_EPOCH + Duration::days(21000);
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://fixture@localhost/identity_test")?;
        let keys = Arc::new(AeadKeyRing::new(
            "unit",
            std::collections::BTreeMap::from([("unit".into(), [7; 32])]),
        )?);
        let clock = Arc::new(FixedClock::new(now));
        assert!(BffStore::new(pool.clone(), "../bad".into(), keys.clone(), clock.clone()).is_err());
        let a = BffStore::new(pool.clone(), "demo-a".into(), keys.clone(), clock.clone())?;
        let b = BffStore::new(pool, "demo-b".into(), keys, clock)?;
        assert_ne!(a.purpose("tokens"), b.purpose("tokens"));
        let mut tokens = BffTokens {
            access_token: "unit-access".into(),
            refresh_token: "unit-refresh".into(),
            id_token: "unit-id".into(),
            access_expires_at: now + Duration::minutes(5),
            session_expires_at: now + Duration::hours(12),
        };
        assert!(validate_tokens(&tokens, now, None).is_ok());
        assert_eq!(format!("{tokens:?}"), "BffTokens([REDACTED])");
        tokens.access_expires_at = now;
        assert!(validate_tokens(&tokens, now, None).is_err());
        Ok(())
    }
}
