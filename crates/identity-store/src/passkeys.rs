//! Persisted WebAuthn ceremonies. No client-supplied library state is ever accepted.
use crate::{
    mfa::{FactorOutcome, LoginCompletion, MfaAudit, require_recent},
    passwords::notification,
    repository::Digest,
    security::{AuditEvent, AuditRecord, AuditResult, AuditTarget, insert_audit},
    sessions::{SessionView, insert_preauth, user_view},
};
use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use identity_core::{
    clock::Clock,
    passkeys::PasskeyEngine,
    security::{AeadEnvelope, AeadKeyRing, constant_time_equal, keyed_account_digest},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use std::{fmt, sync::Arc};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use url::Url;
use uuid::Uuid;

#[derive(Debug, PartialEq, Eq)]
pub enum PasskeyStoreError {
    InvalidSession,
    InvalidChallenge,
    ExpiredChallenge,
    ConsumedChallenge,
    InvalidCredential,
    NotFound,
    LimitReached,
    InvalidName,
    ReauthRequired { strong: bool, methods: Vec<String> },
    Unavailable,
}
impl fmt::Display for PasskeyStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidSession => "session required",
            Self::InvalidChallenge => "invalid passkey challenge",
            Self::ExpiredChallenge => "expired passkey challenge",
            Self::ConsumedChallenge => "consumed passkey challenge",
            Self::InvalidCredential => "invalid passkey credential",
            Self::NotFound => "passkey not found",
            Self::LimitReached => "passkey limit reached",
            Self::InvalidName => "invalid passkey name",
            Self::ReauthRequired { .. } => "recent authentication required",
            Self::Unavailable => "passkey storage unavailable",
        })
    }
}
impl std::error::Error for PasskeyStoreError {}
impl From<sqlx::Error> for PasskeyStoreError {
    fn from(_: sqlx::Error) -> Self {
        Self::Unavailable
    }
}
pub struct PasskeyOptions {
    pub challenge_id: Uuid,
    pub public_key: Value,
    pub expires_at: OffsetDateTime,
}
#[derive(Serialize)]
pub struct CredentialView {
    pub id: Uuid,
    pub name: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
}
#[derive(Serialize)]
pub struct CredentialPage {
    pub items: Vec<CredentialView>,
    pub next_cursor: Option<String>,
}
pub struct RegistrationInput {
    pub session_hash: Digest,
    pub challenge_id: Uuid,
    pub name: String,
    pub credential: Value,
    pub audit: MfaAudit,
}
pub struct PasskeyAssertionInput {
    pub challenge_id: Uuid,
    pub credential: Value,
    pub preauth_hash: Option<Digest>,
    pub session_hash: Option<Digest>,
    pub login: LoginCompletion,
    pub audit: MfaAudit,
    pub reauth_only: bool,
}
#[derive(Clone)]
pub struct PasskeyService {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    keys: Arc<AeadKeyRing>,
    engine: PasskeyEngine,
}
impl PasskeyService {
    pub fn new(
        pool: PgPool,
        clock: Arc<dyn Clock>,
        keys: Arc<AeadKeyRing>,
        issuer: &Url,
        rp_id: &str,
    ) -> Result<Self, PasskeyStoreError> {
        let engine =
            PasskeyEngine::new(issuer, rp_id).map_err(|_| PasskeyStoreError::Unavailable)?;
        Ok(Self {
            pool,
            clock,
            keys,
            engine,
        })
    }
    pub async fn begin_registration(
        &self,
        hash: Digest,
        audit: MfaAudit,
    ) -> Result<PasskeyOptions, PasskeyStoreError> {
        let now = self.clock.now();
        let (mut tx, user, session) = self.lock_session(hash, now).await?;
        let id: Uuid = user.try_get("id")?;
        recent(&mut tx, id, session, now).await?;
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM webauthn_credentials WHERE user_id=$1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
        if count >= 10 {
            return Err(PasskeyStoreError::LimitReached);
        }
        let exclusions: Vec<Vec<u8>> =
            sqlx::query_scalar("SELECT credential_id FROM webauthn_credentials WHERE user_id=$1")
                .bind(id)
                .fetch_all(&mut *tx)
                .await?;
        let (options, state) = self
            .engine
            .registration(id, &user.try_get::<String, _>("email")?, exclusions)
            .map_err(|_| PasskeyStoreError::InvalidCredential)?;
        let result = self
            .persist_challenge(
                &mut tx,
                ChallengeInsert {
                    user: Some(id),
                    session: Some(session),
                    preauth: None,
                    purpose: "passkey_registration",
                    state: &state,
                    version: Some(user.try_get("credential_version")?),
                    now,
                    options,
                },
            )
            .await?;
        let _ = audit;
        tx.commit().await?;
        Ok(result)
    }
    pub async fn finish_registration(
        &self,
        input: &RegistrationInput,
    ) -> Result<CredentialView, PasskeyStoreError> {
        validate_name(&input.name)?;
        let now = self.clock.now();
        let (mut tx, user, session) = self.lock_session(input.session_hash, now).await?;
        let id: Uuid = user.try_get("id")?;
        recent(&mut tx, id, session, now).await?;
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM webauthn_credentials WHERE user_id=$1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
        if count >= 10 {
            return Err(PasskeyStoreError::LimitReached);
        }
        let challenge=sqlx::query("SELECT * FROM authentication_challenges WHERE id=$1 AND user_id=$2 AND session_id=$3 AND purpose='passkey_registration' FOR UPDATE").bind(input.challenge_id).bind(id).bind(session).fetch_optional(&mut *tx).await?.ok_or(PasskeyStoreError::InvalidChallenge)?;
        validate_challenge(&challenge, now, Some(user.try_get("credential_version")?))?;
        let state = self.state(&challenge, id)?;
        let key = match self.engine.verify_registration(&input.credential, &state) {
            Ok(key) => key,
            Err(_) => return failed(tx, input.challenge_id, id, input.audit, now).await,
        };
        let credential_id = self
            .engine
            .credential_id(&key)
            .map_err(|_| PasskeyStoreError::InvalidCredential)?;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM webauthn_credentials WHERE credential_id=$1)",
        )
        .bind(&credential_id)
        .fetch_one(&mut *tx)
        .await?;
        if exists {
            return Err(PasskeyStoreError::InvalidCredential);
        }
        let credential = Uuid::new_v4();
        sqlx::query("INSERT INTO webauthn_credentials(id,user_id,credential_id,credential_data,name,created_at,updated_at) VALUES($1,$2,$3,$4,$5,$6,$6)").bind(credential).bind(id).bind(credential_id).bind(key).bind(&input.name).bind(now).execute(&mut *tx).await?;
        consume(&mut tx, input.challenge_id, now).await?;
        notification(&mut tx, &user, &self.keys, "passkey.registered", now)
            .await
            .map_err(|_| PasskeyStoreError::Unavailable)?;
        audit(
            &mut tx,
            id,
            AuditEvent::PasskeyRegistered,
            credential,
            input.audit,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(CredentialView {
            id: credential,
            name: input.name.clone(),
            created_at: now
                .format(&Rfc3339)
                .map_err(|_| PasskeyStoreError::Unavailable)?,
            last_used_at: None,
        })
    }
    pub async fn begin_login(&self, hash: Digest) -> Result<PasskeyOptions, PasskeyStoreError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let valid=sqlx::query("SELECT id FROM preauthentication_contexts WHERE token_hash=$1 AND revoked_at IS NULL AND expires_at>$2 FOR UPDATE").bind(hash.as_bytes()).bind(now).fetch_optional(&mut *tx).await?;
        if valid.is_none() {
            return Err(PasskeyStoreError::InvalidChallenge);
        }
        let (options, state) = self
            .engine
            .discoverable_options()
            .map_err(|_| PasskeyStoreError::Unavailable)?;
        let result = self
            .persist_challenge(
                &mut tx,
                ChallengeInsert {
                    user: None,
                    session: None,
                    preauth: Some(hash),
                    purpose: "passkey_login",
                    state: &state,
                    version: None,
                    now,
                    options,
                },
            )
            .await?;
        tx.commit().await?;
        Ok(result)
    }
    pub async fn begin_reauthentication(
        &self,
        hash: Digest,
    ) -> Result<PasskeyOptions, PasskeyStoreError> {
        let now = self.clock.now();
        let (mut tx, user, session) = self.lock_session(hash, now).await?;
        let id = user.try_get("id")?;
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM webauthn_credentials WHERE user_id=$1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
        if count == 0 {
            return Err(PasskeyStoreError::NotFound);
        }
        let (options, state) = self
            .engine
            .discoverable_options()
            .map_err(|_| PasskeyStoreError::Unavailable)?;
        let result = self
            .persist_challenge(
                &mut tx,
                ChallengeInsert {
                    user: Some(id),
                    session: Some(session),
                    preauth: None,
                    purpose: "passkey_reauthentication",
                    state: &state,
                    version: Some(user.try_get("credential_version")?),
                    now,
                    options,
                },
            )
            .await?;
        tx.commit().await?;
        Ok(result)
    }
    pub async fn finish_login(
        &self,
        input: &PasskeyAssertionInput,
    ) -> Result<FactorOutcome, PasskeyStoreError> {
        let now = self.clock.now();
        let (id, credential_id) = self
            .engine
            .identify(&input.credential)
            .map_err(|_| PasskeyStoreError::InvalidCredential)?;
        let mut tx = self.pool.begin().await?;
        let user=sqlx::query("SELECT id,email,display_name,verified,status,credential_version,created_at FROM users WHERE id=$1 FOR UPDATE").bind(id).fetch_optional(&mut *tx).await?.ok_or(PasskeyStoreError::InvalidCredential)?;
        if !user.try_get::<bool, _>("verified")? || user.try_get::<String, _>("status")? != "active"
        {
            return Err(PasskeyStoreError::InvalidCredential);
        }
        let located=sqlx::query("SELECT purpose,user_id,session_id,preauth_hash FROM authentication_challenges WHERE id=$1").bind(input.challenge_id).fetch_optional(&mut *tx).await?.ok_or(PasskeyStoreError::InvalidChallenge)?;
        let purpose: String = located.try_get("purpose")?;
        let session: Option<Uuid> = located.try_get("session_id")?;
        if input.reauth_only && purpose != "passkey_reauthentication" {
            return Err(PasskeyStoreError::InvalidChallenge);
        }
        if purpose == "passkey_reauthentication" {
            if located.try_get::<Option<Uuid>, _>("user_id")? != Some(id) {
                return Err(PasskeyStoreError::InvalidCredential);
            }
            let hash = input
                .session_hash
                .ok_or(PasskeyStoreError::InvalidSession)?;
            let valid:Option<Uuid>=sqlx::query_scalar("SELECT id FROM sessions WHERE token_hash=$1 AND user_id=$2 AND credential_version=$3 AND revoked_at IS NULL AND expires_at>$4 FOR UPDATE").bind(hash.as_bytes()).bind(id).bind(user.try_get::<i64,_>("credential_version")?).bind(now).fetch_optional(&mut *tx).await?;
            if valid.is_none() || valid != session {
                return Err(PasskeyStoreError::InvalidSession);
            }
        } else if purpose == "passkey_login" {
            let hash = input
                .preauth_hash
                .ok_or(PasskeyStoreError::InvalidChallenge)?;
            if located
                .try_get::<Option<Vec<u8>>, _>("preauth_hash")?
                .as_deref()
                != Some(hash.as_bytes())
            {
                return Err(PasskeyStoreError::InvalidChallenge);
            }
            let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM preauthentication_contexts WHERE token_hash=$1 AND revoked_at IS NULL AND expires_at>$2)").bind(hash.as_bytes()).bind(now).fetch_one(&mut *tx).await?;
            if !valid {
                return Err(PasskeyStoreError::InvalidChallenge);
            }
        } else {
            return Err(PasskeyStoreError::InvalidChallenge);
        }
        let challenge =
            sqlx::query("SELECT * FROM authentication_challenges WHERE id=$1 FOR UPDATE")
                .bind(input.challenge_id)
                .fetch_one(&mut *tx)
                .await?;
        validate_challenge(
            &challenge,
            now,
            if purpose == "passkey_login" {
                None
            } else {
                Some(user.try_get("credential_version")?)
            },
        )?;
        let credential=sqlx::query("SELECT id,credential_data FROM webauthn_credentials WHERE user_id=$1 AND credential_id=$2 FOR UPDATE").bind(id).bind(&credential_id).fetch_optional(&mut *tx).await?.ok_or(PasskeyStoreError::InvalidCredential)?;
        let state = self.state(&challenge, id)?;
        let key = match self.engine.verify_discoverable(
            &input.credential,
            &state,
            &credential.try_get::<Value, _>("credential_data")?,
        ) {
            Ok(key) => key,
            Err(_) => return failed(tx, input.challenge_id, id, input.audit, now).await,
        };
        sqlx::query("UPDATE webauthn_credentials SET credential_data=$2,updated_at=$3,last_used_at=$3 WHERE id=$1")
            .bind(credential.try_get::<Uuid, _>("id")?)
            .bind(key)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        consume(&mut tx, input.challenge_id, now).await?;
        if purpose == "passkey_reauthentication" {
            sqlx::query("UPDATE sessions SET strong_at=$2 WHERE id=$1")
                .bind(session)
                .bind(now)
                .execute(&mut *tx)
                .await?;
            audit(
                &mut tx,
                id,
                AuditEvent::Reauthenticated,
                credential.try_get("id")?,
                input.audit,
                now,
            )
            .await?;
            tx.commit().await?;
            return Ok(FactorOutcome::Reauthenticated {
                reauthenticated_at: now,
                valid_until: now + Duration::minutes(5),
                amr: vec!["user".into()],
            });
        }
        if input.login.user_agent.chars().count() > 256
            || input.login.user_agent.chars().any(char::is_control)
        {
            return Err(PasskeyStoreError::InvalidCredential);
        }
        let hash = input
            .preauth_hash
            .ok_or(PasskeyStoreError::InvalidChallenge)?;
        let expires_at = now + Duration::hours(12);
        sqlx::query("INSERT INTO sessions(id,token_hash,user_id,amr,auth_time,strong_at,csrf_hash,credential_version,user_agent,created_at,expires_at) VALUES($1,$2,$3,ARRAY['user']::text[],$4,$4,$5,$6,$7,$4,$8)").bind(input.login.session_id).bind(input.login.session_token_hash.as_bytes()).bind(id).bind(now).bind(input.login.session_csrf_hash.as_bytes()).bind(user.try_get::<i64,_>("credential_version")?).bind(&input.login.user_agent).bind(expires_at).execute(&mut *tx).await?;
        sqlx::query("UPDATE authorization_transactions SET preauth_hash=NULL,session_id=$2 WHERE preauth_hash=$1 AND consumed_at IS NULL AND expires_at>$3").bind(hash.as_bytes()).bind(input.login.session_id).bind(now).execute(&mut *tx).await?;
        sqlx::query("UPDATE preauthentication_contexts SET revoked_at=$2 WHERE token_hash=$1")
            .bind(hash.as_bytes())
            .bind(now)
            .execute(&mut *tx)
            .await?;
        insert_preauth(
            &mut tx,
            input.login.new_preauth_hash,
            input.login.new_preauth_csrf_hash,
            now,
        )
        .await
        .map_err(|_| PasskeyStoreError::Unavailable)?;
        let view = user_view(&user).map_err(|_| PasskeyStoreError::Unavailable)?;
        audit(
            &mut tx,
            id,
            AuditEvent::LoginSucceeded,
            input.login.session_id,
            input.audit,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(FactorOutcome::Authenticated {
            user: view,
            session: Box::new(SessionView {
                id: input.login.session_id,
                amr: vec!["user".into()],
                auth_time: now,
                strong_at: Some(now),
                expires_at,
                created_at: now,
                user_agent: input.login.user_agent.clone(),
                current: true,
            }),
        })
    }
    pub async fn list(&self, hash: Digest) -> Result<Vec<CredentialView>, PasskeyStoreError> {
        let now = self.clock.now();
        let (mut tx, user, _) = self.lock_session(hash, now).await?;
        let rows=sqlx::query("SELECT id,name,created_at,last_used_at FROM webauthn_credentials WHERE user_id=$1 ORDER BY created_at DESC,id DESC").bind(user.try_get::<Uuid,_>("id")?).fetch_all(&mut *tx).await?;
        let result = rows.iter().map(view).collect();
        tx.commit().await?;
        result
    }
    pub async fn page(
        &self,
        hash: Digest,
        limit: u32,
        cursor: Option<&str>,
        key: &[u8; 32],
    ) -> Result<CredentialPage, PasskeyStoreError> {
        if !(1..=100).contains(&limit) {
            return Err(PasskeyStoreError::InvalidChallenge);
        }
        let now = self.clock.now();
        let (mut tx, user, _) = self.lock_session(hash, now).await?;
        let id: Uuid = user.try_get("id")?;
        let position = cursor
            .map(|value| parse_cursor(value, id, key, now))
            .transpose()?;
        let created = position.as_ref().map(|value| value.created_at);
        let cursor_id = position.as_ref().map(|value| value.id);
        let rows=sqlx::query("SELECT id,name,created_at,last_used_at FROM webauthn_credentials WHERE user_id=$1 AND ($2::timestamptz IS NULL OR (created_at,id)<($2,$3::uuid)) ORDER BY created_at DESC,id DESC LIMIT $4").bind(id).bind(created).bind(cursor_id).bind(i64::from(limit)+1).fetch_all(&mut *tx).await?;
        let has_more = rows.len() > limit as usize;
        let items = rows
            .iter()
            .take(limit as usize)
            .map(view)
            .collect::<Result<Vec<_>, _>>()?;
        let next_cursor = if has_more {
            let row = &rows[limit as usize - 1];
            Some(make_cursor(
                id,
                row.try_get("created_at")?,
                row.try_get("id")?,
                key,
                now,
            )?)
        } else {
            None
        };
        tx.commit().await?;
        Ok(CredentialPage { items, next_cursor })
    }
    pub async fn get(
        &self,
        hash: Digest,
        target: Uuid,
    ) -> Result<CredentialView, PasskeyStoreError> {
        self.list(hash)
            .await?
            .into_iter()
            .find(|item| item.id == target)
            .ok_or(PasskeyStoreError::NotFound)
    }
    pub async fn rename(
        &self,
        hash: Digest,
        target: Uuid,
        name: &str,
        event: MfaAudit,
    ) -> Result<CredentialView, PasskeyStoreError> {
        validate_name(name)?;
        let now = self.clock.now();
        let (mut tx, user, _) = self.lock_session(hash, now).await?;
        let id = user.try_get("id")?;
        let row=sqlx::query("UPDATE webauthn_credentials SET name=$3,updated_at=$4 WHERE id=$1 AND user_id=$2 RETURNING id,name,created_at,last_used_at").bind(target).bind(id).bind(name).bind(now).fetch_optional(&mut *tx).await?.ok_or(PasskeyStoreError::NotFound)?;
        notification(&mut tx, &user, &self.keys, "passkey.renamed", now)
            .await
            .map_err(|_| PasskeyStoreError::Unavailable)?;
        audit(&mut tx, id, AuditEvent::PasskeyRenamed, target, event, now).await?;
        tx.commit().await?;
        view(&row)
    }
    pub async fn delete(
        &self,
        hash: Digest,
        target: Uuid,
        event: MfaAudit,
    ) -> Result<(), PasskeyStoreError> {
        let now = self.clock.now();
        let (mut tx, user, session) = self.lock_session(hash, now).await?;
        let id = user.try_get("id")?;
        let belongs: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM webauthn_credentials WHERE id=$1 AND user_id=$2)",
        )
        .bind(target)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if !belongs {
            return Err(PasskeyStoreError::NotFound);
        }
        recent(&mut tx, id, session, now).await?;
        sqlx::query("DELETE FROM webauthn_credentials WHERE id=$1 AND user_id=$2")
            .bind(target)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        notification(&mut tx, &user, &self.keys, "passkey.removed", now)
            .await
            .map_err(|_| PasskeyStoreError::Unavailable)?;
        audit(&mut tx, id, AuditEvent::PasskeyDeleted, target, event, now).await?;
        tx.commit().await?;
        Ok(())
    }
    async fn lock_session(
        &self,
        hash: Digest,
        now: OffsetDateTime,
    ) -> Result<(Transaction<'_, Postgres>, PgRow, Uuid), PasskeyStoreError> {
        let id: Option<Uuid> =
            sqlx::query_scalar("SELECT user_id FROM sessions WHERE token_hash=$1")
                .bind(hash.as_bytes())
                .fetch_optional(&self.pool)
                .await?;
        let id = id.ok_or(PasskeyStoreError::InvalidSession)?;
        let mut tx = self.pool.begin().await?;
        let user = sqlx::query(
            "SELECT id,email,verified,status,credential_version FROM users WHERE id=$1 FOR UPDATE",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if !user.try_get::<bool, _>("verified")? || user.try_get::<String, _>("status")? != "active"
        {
            return Err(PasskeyStoreError::InvalidSession);
        }
        let session:Option<Uuid>=sqlx::query_scalar("SELECT id FROM sessions WHERE token_hash=$1 AND user_id=$2 AND credential_version=$3 AND revoked_at IS NULL AND expires_at>$4 FOR UPDATE").bind(hash.as_bytes()).bind(id).bind(user.try_get::<i64,_>("credential_version")?).bind(now).fetch_optional(&mut *tx).await?;
        Ok((tx, user, session.ok_or(PasskeyStoreError::InvalidSession)?))
    }
    async fn persist_challenge(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        input: ChallengeInsert<'_>,
    ) -> Result<PasskeyOptions, PasskeyStoreError> {
        let ChallengeInsert {
            user,
            session,
            preauth,
            purpose,
            state,
            version,
            now,
            options,
        } = input;
        let id = Uuid::new_v4();
        let expires_at = now + Duration::minutes(5);
        let aaduser = user.unwrap_or(Uuid::nil());
        let state = self
            .keys
            .encrypt(aaduser, purpose, state)
            .map_err(|_| PasskeyStoreError::Unavailable)?;
        sqlx::query("INSERT INTO authentication_challenges(id,user_id,session_id,preauth_hash,purpose,state_encrypted,state_data,created_at,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)").bind(id).bind(user).bind(session).bind(preauth.as_ref().map(Digest::as_bytes)).bind(purpose).bind(sqlx::types::Json(state)).bind(serde_json::json!({"credential_version":version})).bind(now).bind(expires_at).execute(&mut **tx).await?;
        Ok(PasskeyOptions {
            challenge_id: id,
            public_key: options["publicKey"].clone(),
            expires_at,
        })
    }
    fn state(
        &self,
        challenge: &PgRow,
        user: Uuid,
    ) -> Result<zeroize::Zeroizing<Vec<u8>>, PasskeyStoreError> {
        let purpose: String = challenge.try_get("purpose")?;
        let actual = if purpose == "passkey_login" {
            Uuid::nil()
        } else {
            user
        };
        let envelope: AeadEnvelope =
            serde_json::from_value(challenge.try_get::<Value, _>("state_encrypted")?)
                .map_err(|_| PasskeyStoreError::InvalidChallenge)?;
        self.keys
            .decrypt(actual, &purpose, &envelope)
            .map_err(|_| PasskeyStoreError::Unavailable)
    }
}
async fn recent(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
    session: Uuid,
    now: OffsetDateTime,
) -> Result<(), PasskeyStoreError> {
    require_recent(tx, user, session, now)
        .await
        .map_err(|error| match error {
            crate::mfa::MfaError::ReauthRequired { strong, methods } => {
                PasskeyStoreError::ReauthRequired { strong, methods }
            }
            _ => PasskeyStoreError::Unavailable,
        })
}
fn validate_name(name: &str) -> Result<(), PasskeyStoreError> {
    if name.trim().is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
        Err(PasskeyStoreError::InvalidName)
    } else {
        Ok(())
    }
}
fn validate_challenge(
    row: &PgRow,
    now: OffsetDateTime,
    version: Option<i64>,
) -> Result<(), PasskeyStoreError> {
    if row
        .try_get::<Option<OffsetDateTime>, _>("consumed_at")?
        .is_some()
    {
        return Err(PasskeyStoreError::ConsumedChallenge);
    }
    if row.try_get::<OffsetDateTime, _>("expires_at")? <= now {
        return Err(PasskeyStoreError::ExpiredChallenge);
    }
    if row.try_get::<i32, _>("attempts")? >= 5 {
        return Err(PasskeyStoreError::ConsumedChallenge);
    }
    if let Some(version) = version {
        let state: Value = row.try_get("state_data")?;
        if state.get("credential_version").and_then(Value::as_i64) != Some(version) {
            return Err(PasskeyStoreError::InvalidChallenge);
        }
    }
    Ok(())
}
async fn consume(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    now: OffsetDateTime,
) -> Result<(), PasskeyStoreError> {
    sqlx::query(
        "UPDATE authentication_challenges SET consumed_at=$2,state_encrypted=NULL WHERE id=$1",
    )
    .bind(id)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
async fn failed<T>(
    mut tx: Transaction<'_, Postgres>,
    id: Uuid,
    user: Uuid,
    context: MfaAudit,
    now: OffsetDateTime,
) -> Result<T, PasskeyStoreError> {
    let attempts:i32=sqlx::query_scalar("UPDATE authentication_challenges SET attempts=attempts+1,consumed_at=CASE WHEN attempts+1>=5 THEN $2 ELSE NULL END,state_encrypted=CASE WHEN attempts+1>=5 THEN NULL ELSE state_encrypted END WHERE id=$1 RETURNING attempts").bind(id).bind(now).fetch_one(&mut *tx).await?;
    audit(
        &mut tx,
        user,
        if attempts >= 5 {
            AuditEvent::ChallengeExhausted
        } else {
            AuditEvent::ChallengeFailed
        },
        id,
        context,
        now,
    )
    .await?;
    tx.commit().await?;
    Err(PasskeyStoreError::InvalidCredential)
}
struct ChallengeInsert<'a> {
    user: Option<Uuid>,
    session: Option<Uuid>,
    preauth: Option<Digest>,
    purpose: &'a str,
    state: &'a [u8],
    version: Option<i64>,
    now: OffsetDateTime,
    options: Value,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PasskeyCursor {
    user: Uuid,
    id: Uuid,
    created_at: OffsetDateTime,
    expires_at: OffsetDateTime,
    route: String,
}
fn make_cursor(
    user: Uuid,
    created_at: OffsetDateTime,
    id: Uuid,
    key: &[u8; 32],
    now: OffsetDateTime,
) -> Result<String, PasskeyStoreError> {
    let payload = PasskeyCursor {
        user,
        id,
        created_at,
        expires_at: now + Duration::minutes(10),
        route: "me/passkeys:created_desc,id_desc".into(),
    };
    let json = serde_json::to_string(&payload).map_err(|_| PasskeyStoreError::InvalidChallenge)?;
    let mac = keyed_account_digest(key, "passkey-cursor-v1", &json);
    Ok(format!(
        "{}.{}",
        BASE64_URL_SAFE_NO_PAD.encode(json),
        BASE64_URL_SAFE_NO_PAD.encode(mac)
    ))
}
fn parse_cursor(
    value: &str,
    user: Uuid,
    key: &[u8; 32],
    now: OffsetDateTime,
) -> Result<PasskeyCursor, PasskeyStoreError> {
    if value.len() > 2048 {
        return Err(PasskeyStoreError::InvalidChallenge);
    }
    let (payload, mac) = value
        .split_once('.')
        .ok_or(PasskeyStoreError::InvalidChallenge)?;
    let raw = BASE64_URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| PasskeyStoreError::InvalidChallenge)?;
    let json = std::str::from_utf8(&raw).map_err(|_| PasskeyStoreError::InvalidChallenge)?;
    let mac = BASE64_URL_SAFE_NO_PAD
        .decode(mac)
        .map_err(|_| PasskeyStoreError::InvalidChallenge)?;
    if !constant_time_equal(&mac, &keyed_account_digest(key, "passkey-cursor-v1", json)) {
        return Err(PasskeyStoreError::InvalidChallenge);
    }
    let cursor: PasskeyCursor =
        serde_json::from_str(json).map_err(|_| PasskeyStoreError::InvalidChallenge)?;
    if cursor.user != user
        || cursor.expires_at <= now
        || cursor.route != "me/passkeys:created_desc,id_desc"
    {
        return Err(PasskeyStoreError::InvalidChallenge);
    }
    Ok(cursor)
}
fn view(row: &PgRow) -> Result<CredentialView, PasskeyStoreError> {
    Ok(CredentialView {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        created_at: row
            .try_get::<OffsetDateTime, _>("created_at")?
            .format(&Rfc3339)
            .map_err(|_| PasskeyStoreError::Unavailable)?,
        last_used_at: row
            .try_get::<Option<OffsetDateTime>, _>("last_used_at")?
            .map(|value| value.format(&Rfc3339))
            .transpose()
            .map_err(|_| PasskeyStoreError::Unavailable)?,
    })
}
async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
    event: AuditEvent,
    target: Uuid,
    ctx: MfaAudit,
    now: OffsetDateTime,
) -> Result<(), PasskeyStoreError> {
    insert_audit(
        tx,
        &AuditRecord {
            id: Uuid::new_v4(),
            event,
            actor_id: Some(user),
            target: AuditTarget::Credential,
            target_id: Some(target),
            result: if matches!(
                event,
                AuditEvent::ChallengeFailed | AuditEvent::ChallengeExhausted
            ) {
                AuditResult::Failure
            } else {
                AuditResult::Success
            },
            request_id: ctx.request_id,
            source_hash: ctx.source_hash,
            occurred_at: now,
        },
    )
    .await
    .map_err(|_| PasskeyStoreError::Unavailable)
}
