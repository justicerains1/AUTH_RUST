//! Password mutation transactions: hash work happens before locks; SMTP happens after commits.
use crate::{
    repository::Digest,
    security::{AuditEvent, AuditRecord, AuditResult, AuditTarget, insert_audit},
    sessions::{LoginCredential, lock_revocation, revoke_locked},
};
use identity_core::{
    clock::Clock,
    security::{AeadEnvelope, AeadKeyRing, Token, token_digest},
};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use std::{fmt, sync::Arc};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

#[derive(Debug, PartialEq, Eq)]
pub enum PasswordError {
    InvalidSession,
    InvalidCredentials,
    InvalidAction,
    ExpiredAction,
    ConsumedAction,
    ReauthRequired { strong: bool, methods: Vec<String> },
    Unavailable,
}
impl fmt::Display for PasswordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidSession => "session required",
            Self::InvalidCredentials => "invalid credentials",
            Self::InvalidAction => "invalid password action",
            Self::ExpiredAction => "expired password action",
            Self::ConsumedAction => "consumed password action",
            Self::ReauthRequired { .. } => "recent authentication required",
            Self::Unavailable => "password storage unavailable",
        })
    }
}
impl std::error::Error for PasswordError {}
impl From<sqlx::Error> for PasswordError {
    fn from(_: sqlx::Error) -> Self {
        Self::Unavailable
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetMail {
    pub reset_url: String,
    pub expires_at: String,
}
impl Drop for ResetMail {
    fn drop(&mut self) {
        self.reset_url.zeroize();
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityNotificationMail {
    pub event: String,
    pub occurred_at: String,
}
pub struct ResetCommitInput {
    pub token_hash: Digest,
    pub new_password_hash: Zeroizing<String>,
    pub source_hash: Digest,
    pub request_id: Uuid,
}
pub struct ChangeCommitInput {
    pub session_hash: Digest,
    pub expected_credential_version: i64,
    pub new_password_hash: Zeroizing<String>,
    pub source_hash: Digest,
    pub request_id: Uuid,
}
pub struct ReauthCommitInput {
    pub session_hash: Digest,
    pub expected_credential_version: i64,
    pub source_hash: Digest,
    pub request_id: Uuid,
}
pub enum ReauthOutcome {
    Confirmed {
        reauthenticated_at: OffsetDateTime,
        valid_until: OffsetDateTime,
        amr: Vec<String>,
    },
    MfaRequired {
        challenge_id: Uuid,
        expires_at: OffsetDateTime,
        methods: Vec<String>,
    },
}

#[derive(Clone)]
pub struct PasswordStore {
    pool: PgPool,
    clock: Arc<dyn Clock>,
}
impl PasswordStore {
    pub fn new(pool: PgPool, clock: Arc<dyn Clock>) -> Self {
        Self { pool, clock }
    }
    pub async fn request_reset(
        &self,
        email: &str,
        issuer: &str,
        keys: &AeadKeyRing,
        source: Digest,
        request_id: Uuid,
    ) -> Result<(), PasswordError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let user = sqlx::query("SELECT id,email FROM users WHERE email=$1 FOR UPDATE")
            .bind(email)
            .fetch_optional(&mut *tx)
            .await?;
        let Some(user) = user else {
            tx.rollback().await?;
            return Ok(());
        };
        let id: Uuid = user.try_get("id")?;
        sqlx::query("SELECT id FROM email_actions WHERE user_id=$1 AND purpose='reset' ORDER BY id FOR UPDATE").bind(id).fetch_all(&mut *tx).await?;
        sqlx::query("UPDATE email_actions SET consumed_at=COALESCE(consumed_at,$2) WHERE user_id=$1 AND purpose='reset'").bind(id).bind(now).execute(&mut *tx).await?;
        let token = Token::generate().map_err(|_| PasswordError::Unavailable)?;
        let expires = now + Duration::minutes(15);
        let payload = ResetMail {
            reset_url: format!(
                "{}/password-reset#token={}",
                issuer.trim_end_matches('/'),
                token.expose()
            ),
            expires_at: expires
                .format(&Rfc3339)
                .map_err(|_| PasswordError::Unavailable)?,
        };
        let envelope = encrypt_mail(keys, id, &payload)?;
        sqlx::query("INSERT INTO email_actions(id,user_id,token_hash,purpose,created_at,expires_at) VALUES($1,$2,$3,'reset',$4,$5)").bind(Uuid::new_v4()).bind(id).bind(token_digest(token.expose()).as_slice()).bind(now).bind(expires).execute(&mut *tx).await?;
        outbox(&mut tx, id, email, "reset_password", envelope, now).await?;
        audit(
            &mut tx,
            id,
            AuditEvent::PasswordResetRequested,
            AuditContext {
                source,
                request_id,
                now,
            },
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn credential_by_session(
        &self,
        hash: Digest,
    ) -> Result<Option<LoginCredential>, PasswordError> {
        let row=sqlx::query("SELECT u.id,u.email,u.password_hash,u.verified,u.status,u.credential_version,EXISTS(SELECT 1 FROM totp_factors f WHERE f.user_id=u.id AND f.confirmed) AS totp_enabled FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=$1 AND s.revoked_at IS NULL AND s.expires_at>$2 AND s.credential_version=u.credential_version AND u.verified AND u.status='active'").bind(hash.as_bytes()).bind(self.clock.now()).fetch_optional(&self.pool).await?;
        row.map(|row| {
            Ok(LoginCredential {
                id: row.try_get("id")?,
                email: row.try_get("email")?,
                password_hash: Zeroizing::new(row.try_get("password_hash")?),
                verified: row.try_get("verified")?,
                status: row.try_get("status")?,
                credential_version: row.try_get("credential_version")?,
                totp_enabled: row.try_get("totp_enabled")?,
            })
        })
        .transpose()
    }
    pub async fn confirm_password_reauthentication(
        &self,
        input: &ReauthCommitInput,
    ) -> Result<ReauthOutcome, PasswordError> {
        let now = self.clock.now();
        let (mut tx, user, session) = self
            .lock_session(input.session_hash, input.expected_credential_version, now)
            .await?;
        let id: Uuid = user.try_get("id")?;
        let methods = factor_methods(&mut tx, id).await?;
        if methods.is_empty() {
            sqlx::query("UPDATE sessions SET password_confirmed_at=$2 WHERE id=$1")
                .bind(session)
                .bind(now)
                .execute(&mut *tx)
                .await?;
            audit(
                &mut tx,
                id,
                AuditEvent::Reauthenticated,
                AuditContext {
                    source: input.source_hash,
                    request_id: input.request_id,
                    now,
                },
            )
            .await?;
            tx.commit().await?;
            Ok(ReauthOutcome::Confirmed {
                reauthenticated_at: now,
                valid_until: now + Duration::minutes(5),
                amr: vec!["pwd".into()],
            })
        } else {
            let challenge_methods: Vec<String> = methods
                .iter()
                .filter(|method| method.as_str() != "passkey")
                .cloned()
                .collect();
            if challenge_methods.is_empty() {
                return Err(PasswordError::ReauthRequired {
                    strong: true,
                    methods,
                });
            }
            let challenge_id = Uuid::new_v4();
            let expires_at = now + Duration::minutes(5);
            sqlx::query("SELECT id FROM authentication_challenges WHERE user_id=$1 AND session_id=$2 AND purpose='reauthentication' ORDER BY id FOR UPDATE").bind(id).bind(session).fetch_all(&mut *tx).await?;
            sqlx::query("UPDATE authentication_challenges SET consumed_at=COALESCE(consumed_at,$3) WHERE user_id=$1 AND session_id=$2 AND purpose='reauthentication'").bind(id).bind(session).bind(now).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO authentication_challenges(id,user_id,session_id,purpose,state_data,created_at,expires_at) VALUES($1,$2,$3,'reauthentication',$4,$5,$6)").bind(challenge_id).bind(id).bind(session).bind(serde_json::json!({"credential_version":input.expected_credential_version})).bind(now).bind(expires_at).execute(&mut *tx).await?;
            tx.commit().await?;
            Ok(ReauthOutcome::MfaRequired {
                challenge_id,
                expires_at,
                methods: challenge_methods,
            })
        }
    }
    pub async fn confirm_reset(
        &self,
        input: &ResetCommitInput,
        keys: &AeadKeyRing,
    ) -> Result<(), PasswordError> {
        let now = self.clock.now();
        let id: Option<Uuid> = sqlx::query_scalar(
            "SELECT user_id FROM email_actions WHERE token_hash=$1 AND purpose='reset'",
        )
        .bind(input.token_hash.as_bytes())
        .fetch_optional(&self.pool)
        .await?;
        let id = id.ok_or(PasswordError::InvalidAction)?;
        let mut tx = self.pool.begin().await?;
        let user =
            sqlx::query("SELECT id,email,credential_version FROM users WHERE id=$1 FOR UPDATE")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
        let sessions: Vec<Uuid> =
            sqlx::query_scalar("SELECT id FROM sessions WHERE user_id=$1 ORDER BY id")
                .bind(id)
                .fetch_all(&mut *tx)
                .await?;
        lock_revocation(&mut tx, id, &sessions)
            .await
            .map_err(|_| PasswordError::Unavailable)?;
        let action=sqlx::query("SELECT id,expires_at,consumed_at FROM email_actions WHERE token_hash=$1 AND user_id=$2 AND purpose='reset' FOR UPDATE").bind(input.token_hash.as_bytes()).bind(id).fetch_optional(&mut *tx).await?.ok_or(PasswordError::InvalidAction)?;
        if action
            .try_get::<Option<OffsetDateTime>, _>("consumed_at")?
            .is_some()
        {
            return Err(PasswordError::ConsumedAction);
        }
        if action.try_get::<OffsetDateTime, _>("expires_at")? <= now {
            return Err(PasswordError::ExpiredAction);
        }
        sqlx::query("UPDATE email_actions SET consumed_at=$2 WHERE id=$1")
            .bind(action.try_get::<Uuid, _>("id")?)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        mutate_password(&mut tx, id, &input.new_password_hash, now).await?;
        revoke_locked(&mut tx, id, &sessions, now)
            .await
            .map_err(|_| PasswordError::Unavailable)?;
        notification(&mut tx, &user, keys, "password.reset_completed", now).await?;
        audit(
            &mut tx,
            id,
            AuditEvent::PasswordReset,
            AuditContext {
                source: input.source_hash,
                request_id: input.request_id,
                now,
            },
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn change_password(
        &self,
        input: &ChangeCommitInput,
        keys: &AeadKeyRing,
    ) -> Result<(), PasswordError> {
        let now = self.clock.now();
        let (mut tx, user, current) = self
            .lock_session(input.session_hash, input.expected_credential_version, now)
            .await?;
        let id: Uuid = user.try_get("id")?;
        let methods = factor_methods(&mut tx, id).await?;
        let strong = !methods.is_empty();
        let session =
            sqlx::query("SELECT password_confirmed_at,strong_at FROM sessions WHERE id=$1")
                .bind(current)
                .fetch_one(&mut *tx)
                .await?;
        let confirmed: Option<OffsetDateTime> = session.try_get(if strong {
            "strong_at"
        } else {
            "password_confirmed_at"
        })?;
        if confirmed.is_none_or(|at| at > now || at + Duration::minutes(5) <= now) {
            return Err(PasswordError::ReauthRequired {
                strong,
                methods: if strong {
                    methods
                } else {
                    vec!["password".into()]
                },
            });
        }
        let ids: Vec<Uuid> =
            sqlx::query_scalar("SELECT id FROM sessions WHERE user_id=$1 ORDER BY id")
                .bind(id)
                .fetch_all(&mut *tx)
                .await?;
        lock_revocation(&mut tx, id, &ids)
            .await
            .map_err(|_| PasswordError::Unavailable)?;
        mutate_password(&mut tx, id, &input.new_password_hash, now).await?;
        revoke_locked(&mut tx, id, &ids, now)
            .await
            .map_err(|_| PasswordError::Unavailable)?;
        notification(&mut tx, &user, keys, "password.changed", now).await?;
        audit(
            &mut tx,
            id,
            AuditEvent::PasswordChanged,
            AuditContext {
                source: input.source_hash,
                request_id: input.request_id,
                now,
            },
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
    async fn lock_session(
        &self,
        hash: Digest,
        expected: i64,
        now: OffsetDateTime,
    ) -> Result<(Transaction<'_, Postgres>, PgRow, Uuid), PasswordError> {
        let user: Option<Uuid> =
            sqlx::query_scalar("SELECT user_id FROM sessions WHERE token_hash=$1")
                .bind(hash.as_bytes())
                .fetch_optional(&self.pool)
                .await?;
        let id = user.ok_or(PasswordError::InvalidSession)?;
        let mut tx = self.pool.begin().await?;
        let user = sqlx::query(
            "SELECT id,email,verified,status,credential_version FROM users WHERE id=$1 FOR UPDATE",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if !user.try_get::<bool, _>("verified")?
            || user.try_get::<String, _>("status")? != "active"
            || user.try_get::<i64, _>("credential_version")? != expected
        {
            return Err(PasswordError::InvalidCredentials);
        }
        let session:Option<Uuid>=sqlx::query_scalar("SELECT id FROM sessions WHERE token_hash=$1 AND user_id=$2 AND credential_version=$3 AND revoked_at IS NULL AND expires_at>$4 FOR UPDATE").bind(hash.as_bytes()).bind(id).bind(expected).bind(now).fetch_optional(&mut *tx).await?;
        Ok((tx, user, session.ok_or(PasswordError::InvalidSession)?))
    }
}
async fn factor_methods(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<Vec<String>, PasswordError> {
    let row=sqlx::query("SELECT EXISTS(SELECT 1 FROM totp_factors WHERE user_id=$1 AND confirmed) AS totp,EXISTS(SELECT 1 FROM webauthn_credentials WHERE user_id=$1) AS passkey,EXISTS(SELECT 1 FROM recovery_codes WHERE user_id=$1 AND consumed_at IS NULL) AS recovery").bind(id).fetch_one(&mut **tx).await?;
    let mut methods = Vec::new();
    if row.try_get::<bool, _>("totp")? {
        methods.push("totp".into());
    }
    if row.try_get::<bool, _>("passkey")? {
        methods.push("passkey".into());
    }
    if row.try_get::<bool, _>("recovery")? {
        methods.push("recovery_code".into());
    }
    Ok(methods)
}
fn encrypt_mail<T: Serialize>(
    keys: &AeadKeyRing,
    user: Uuid,
    payload: &T,
) -> Result<sqlx::types::Json<AeadEnvelope>, PasswordError> {
    let raw = Zeroizing::new(serde_json::to_vec(payload).map_err(|_| PasswordError::Unavailable)?);
    keys.encrypt(user, "email-outbox", &raw)
        .map(sqlx::types::Json)
        .map_err(|_| PasswordError::Unavailable)
}
async fn outbox(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
    email: &str,
    template: &str,
    envelope: sqlx::types::Json<AeadEnvelope>,
    now: OffsetDateTime,
) -> Result<(), PasswordError> {
    sqlx::query("INSERT INTO email_outbox(id,user_id,recipient,template,encrypted_params,created_at,next_attempt_at) VALUES($1,$2,$3,$4,$5,$6,$6)").bind(Uuid::new_v4()).bind(user).bind(email).bind(template).bind(envelope).bind(now).execute(&mut **tx).await?;
    Ok(())
}
async fn notification(
    tx: &mut Transaction<'_, Postgres>,
    user: &PgRow,
    keys: &AeadKeyRing,
    event: &str,
    now: OffsetDateTime,
) -> Result<(), PasswordError> {
    let id = user.try_get("id")?;
    let payload = SecurityNotificationMail {
        event: event.into(),
        occurred_at: now
            .format(&Rfc3339)
            .map_err(|_| PasswordError::Unavailable)?,
    };
    outbox(
        tx,
        id,
        &user.try_get::<String, _>("email")?,
        "security_notification",
        encrypt_mail(keys, id, &payload)?,
        now,
    )
    .await
}
async fn mutate_password(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
    hash: &str,
    now: OffsetDateTime,
) -> Result<(), PasswordError> {
    sqlx::query("UPDATE users SET password_hash=$2,credential_version=credential_version+1,updated_at=$3 WHERE id=$1").bind(user).bind(hash).bind(now).execute(&mut **tx).await?;
    sqlx::query("UPDATE authentication_challenges SET consumed_at=COALESCE(consumed_at,$2) WHERE user_id=$1").bind(user).bind(now).execute(&mut **tx).await?;
    Ok(())
}
struct AuditContext {
    source: Digest,
    request_id: Uuid,
    now: OffsetDateTime,
}
async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
    event: AuditEvent,
    context: AuditContext,
) -> Result<(), PasswordError> {
    insert_audit(
        tx,
        &AuditRecord {
            id: Uuid::new_v4(),
            event,
            actor_id: Some(user),
            target: AuditTarget::User,
            target_id: Some(user),
            result: AuditResult::Success,
            request_id: context.request_id,
            source_hash: context.source,
            occurred_at: context.now,
        },
    )
    .await
    .map_err(|_| PasswordError::Unavailable)
}
