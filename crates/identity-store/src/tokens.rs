//! Authorization-code exchange signs before the single authority commit.
use crate::{
    repository::{Digest, Repository},
    security::{AuditEvent, AuditRecord, AuditResult, AuditTarget, insert_audit},
};
use identity_core::{
    clock::Clock,
    jose::{IdTokenClaims, Signer},
    oauth::pkce_s256,
    security::{Token, constant_time_equal, token_digest},
};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use std::{fmt, sync::Arc};
use time::Duration;
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenError {
    InvalidClient,
    InvalidGrant,
    InvalidToken,
    Unavailable,
    SigningFailed,
}
impl fmt::Display for TokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidClient => "invalid_client",
            Self::InvalidGrant => "invalid_grant",
            Self::InvalidToken => "invalid_token",
            Self::Unavailable | Self::SigningFailed => "temporarily_unavailable",
        })
    }
}
impl std::error::Error for TokenError {}
impl From<sqlx::Error> for TokenError {
    fn from(_: sqlx::Error) -> Self {
        Self::Unavailable
    }
}
#[derive(Clone)]
pub struct ClientIdentity {
    id: Uuid,
    client_id: String,
}
impl ClientIdentity {
    pub fn id(&self) -> Uuid {
        self.id
    }
    pub fn client_id(&self) -> &str {
        &self.client_id
    }
}
pub struct ExchangeInput {
    pub client: ClientIdentity,
    pub code_hash: Digest,
    pub redirect_uri: String,
    pub code_verifier: Zeroizing<String>,
    pub request_id: Uuid,
    pub source_hash: Digest,
}
pub struct TokenBundle {
    pub access_token: Token,
    pub refresh_token: Token,
    pub id_token: Zeroizing<String>,
    pub expires_in: u64,
    pub scope: String,
}
#[derive(Clone)]
pub struct TokenStore {
    pool: PgPool,
    clock: Arc<dyn Clock>,
}
impl TokenStore {
    pub fn new(pool: PgPool, clock: Arc<dyn Clock>) -> Self {
        Self { pool, clock }
    }
    pub async fn authenticate_client(
        &self,
        client_id: &str,
        secret: &str,
    ) -> Result<ClientIdentity, TokenError> {
        let row = sqlx::query(
            "SELECT id,client_id,secret_hash FROM oauth_clients WHERE client_id=$1 AND enabled",
        )
        .bind(client_id)
        .fetch_optional(&self.pool)
        .await?;
        let expected = token_digest(secret);
        let Some(row) = row else {
            let _ = constant_time_equal(&expected, &[0; 32]);
            return Err(TokenError::InvalidClient);
        };
        let actual: Vec<u8> = row.try_get("secret_hash")?;
        if !constant_time_equal(&actual, &expected) {
            return Err(TokenError::InvalidClient);
        }
        Ok(ClientIdentity {
            id: row.try_get("id")?,
            client_id: row.try_get("client_id")?,
        })
    }
    pub async fn exchange_authorization_code(
        &self,
        input: &ExchangeInput,
        signer: &Signer,
    ) -> Result<TokenBundle, TokenError> {
        let now = self.clock.now();
        let challenge = pkce_s256(&input.code_verifier).map_err(|_| TokenError::InvalidGrant)?;
        let binding=sqlx::query("SELECT g.user_id,g.session_id,g.id AS grant_id FROM authorization_codes c JOIN oauth_grants g ON g.id=c.grant_id WHERE c.code_hash=$1 AND g.client_id=$2").bind(input.code_hash.as_bytes()).bind(input.client.id).fetch_optional(&self.pool).await?.ok_or(TokenError::InvalidGrant)?;
        let user: Uuid = binding.try_get("user_id")?;
        let session: Uuid = binding.try_get("session_id")?;
        let grant: Uuid = binding.try_get("grant_id")?;
        let mut tx = self.pool.begin().await?;
        let user_state = sqlx::query(
            "SELECT verified,status,credential_version FROM users WHERE id=$1 FOR UPDATE",
        )
        .bind(user)
        .fetch_one(&mut *tx)
        .await?;
        if !user_state.try_get::<bool, _>("verified")?
            || user_state.try_get::<String, _>("status")? != "active"
        {
            return Err(TokenError::InvalidGrant);
        }
        let session_state=sqlx::query("SELECT amr,auth_time,expires_at FROM sessions WHERE id=$1 AND user_id=$2 AND credential_version=$3 AND revoked_at IS NULL AND expires_at>$4 FOR UPDATE").bind(session).bind(user).bind(user_state.try_get::<i64,_>("credential_version")?).bind(now).fetch_optional(&mut *tx).await?.ok_or(TokenError::InvalidGrant)?;
        let grant_state=sqlx::query("SELECT scopes,expires_at FROM oauth_grants WHERE id=$1 AND user_id=$2 AND session_id=$3 AND client_id=$4 AND revoked_at IS NULL AND expires_at>$5 FOR UPDATE").bind(grant).bind(user).bind(session).bind(input.client.id).bind(now).fetch_optional(&mut *tx).await?.ok_or(TokenError::InvalidGrant)?;
        let enabled: bool =
            sqlx::query_scalar("SELECT enabled FROM oauth_clients WHERE id=$1 FOR SHARE")
                .bind(input.client.id)
                .fetch_one(&mut *tx)
                .await?;
        if !enabled {
            return Err(TokenError::InvalidClient);
        }
        let code=sqlx::query("SELECT id,nonce,code_challenge,redirect_uri FROM authorization_codes WHERE code_hash=$1 AND grant_id=$2 AND consumed_at IS NULL AND expires_at>$3 AND code_challenge_method='S256' FOR UPDATE").bind(input.code_hash.as_bytes()).bind(grant).bind(now).fetch_optional(&mut *tx).await?.ok_or(TokenError::InvalidGrant)?;
        if code.try_get::<String, _>("redirect_uri")? != input.redirect_uri
            || !constant_time_equal(
                code.try_get::<String, _>("code_challenge")?.as_bytes(),
                challenge.as_bytes(),
            )
        {
            return Err(TokenError::InvalidGrant);
        }
        let session_expiry = session_state.try_get::<time::OffsetDateTime, _>("expires_at")?;
        let grant_expiry = grant_state.try_get::<time::OffsetDateTime, _>("expires_at")?;
        let family_expiry = session_expiry.min(grant_expiry);
        let access_expiry = (now + Duration::minutes(5)).min(family_expiry);
        let access = Token::generate().map_err(|_| TokenError::Unavailable)?;
        let refresh = Token::generate().map_err(|_| TokenError::Unavailable)?;
        let family = Uuid::new_v4();
        let scopes: Vec<String> = grant_state.try_get("scopes")?;
        let claims = IdTokenClaims {
            iss: signer.issuer().into(),
            sub: user,
            aud: input.client.client_id.clone(),
            exp: access_expiry.unix_timestamp(),
            iat: now.unix_timestamp(),
            nonce: Some(code.try_get("nonce")?),
            auth_time: session_state
                .try_get::<time::OffsetDateTime, _>("auth_time")?
                .unix_timestamp(),
            amr: session_state.try_get("amr")?,
            sid: session,
        };
        let id_token = signer
            .sign(&claims)
            .map_err(|_| TokenError::SigningFailed)?;
        for (kind, token, expires) in [
            ("access", &access, access_expiry),
            ("refresh", &refresh, family_expiry),
        ] {
            sqlx::query("INSERT INTO oauth_tokens(id,token_hash,kind,grant_id,family_id,family_expires_at,expires_at,created_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8)").bind(Uuid::new_v4()).bind(token_digest(token.expose()).as_slice()).bind(kind).bind(grant).bind(family).bind(family_expiry).bind(expires).bind(now).execute(&mut *tx).await?;
        }
        sqlx::query("UPDATE authorization_codes SET consumed_at=$2 WHERE id=$1")
            .bind(code.try_get::<Uuid, _>("id")?)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        insert_audit(
            &mut tx,
            &AuditRecord {
                id: Uuid::new_v4(),
                event: AuditEvent::CodeConsumed,
                actor_id: Some(user),
                target: AuditTarget::Grant,
                target_id: Some(grant),
                result: AuditResult::Success,
                request_id: input.request_id,
                source_hash: input.source_hash,
                occurred_at: now,
            },
        )
        .await
        .map_err(|_| TokenError::Unavailable)?;
        tx.commit().await?;
        Ok(TokenBundle {
            access_token: access,
            refresh_token: refresh,
            id_token,
            expires_in: u64::try_from((access_expiry - now).whole_seconds())
                .map_err(|_| TokenError::InvalidGrant)?,
            scope: scopes.join(" "),
        })
    }
    pub async fn userinfo(&self, hash: Digest) -> Result<Value, TokenError> {
        let now = self.clock.now();
        let row=sqlx::query("SELECT u.id,u.email,u.verified,u.display_name,g.scopes FROM oauth_tokens t JOIN oauth_grants g ON g.id=t.grant_id JOIN sessions s ON s.id=g.session_id JOIN users u ON u.id=g.user_id AND u.id=s.user_id JOIN oauth_clients c ON c.id=g.client_id WHERE t.token_hash=$1 AND t.kind='access' AND t.consumed_at IS NULL AND t.revoked_at IS NULL AND t.expires_at>$2 AND g.revoked_at IS NULL AND g.expires_at>$2 AND s.revoked_at IS NULL AND s.expires_at>$2 AND s.credential_version=u.credential_version AND u.verified AND u.status='active' AND c.enabled").bind(hash.as_bytes()).bind(now).fetch_optional(&self.pool).await?.ok_or(TokenError::InvalidToken)?;
        let scopes: Vec<String> = row.try_get("scopes")?;
        let mut claims = json!({"sub":row.try_get::<Uuid,_>("id")?});
        if scopes.iter().any(|s| s == "email") {
            claims["email"] = Value::String(row.try_get("email")?);
            claims["email_verified"] = Value::Bool(row.try_get("verified")?);
        }
        if scopes.iter().any(|s| s == "profile")
            && let Some(name) = row.try_get::<Option<String>, _>("display_name")?
        {
            claims["display_name"] = Value::String(name);
        }
        Ok(claims)
    }
    pub fn authority_repository(&self) -> Repository {
        Repository::new(self.pool.clone(), self.clock.clone())
    }
}
