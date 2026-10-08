//! Atomic factor ceremonies and purpose-bound challenge consumption; failures commit their budget.
use crate::{
    passwords::{factor_methods, notification},
    repository::Digest,
    security::{AuditEvent, AuditRecord, AuditResult, AuditTarget, insert_audit},
    sessions::{SessionView, UserView, insert_preauth, user_view},
};
use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use identity_core::{
    clock::Clock,
    mfa::{RecoveryCodes, TotpSecret},
    security::{AeadEnvelope, AeadKeyRing, token_digest},
};
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use std::{fmt, sync::Arc};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Debug, PartialEq, Eq)]
pub enum MfaError {
    InvalidSession,
    InvalidChallenge,
    ExpiredChallenge,
    ConsumedChallenge,
    InvalidCode,
    AttemptsExhausted,
    AlreadyConfigured,
    LastAdministrator,
    ReauthRequired { strong: bool, methods: Vec<String> },
    Unavailable,
}
impl fmt::Display for MfaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidSession => "session required",
            Self::InvalidChallenge => "invalid factor challenge",
            Self::ExpiredChallenge => "expired factor challenge",
            Self::ConsumedChallenge => "consumed factor challenge",
            Self::InvalidCode => "invalid factor code",
            Self::AttemptsExhausted => "factor challenge attempt budget exhausted",
            Self::AlreadyConfigured => "factor already configured",
            Self::LastAdministrator => "last available administrator factor is protected",
            Self::ReauthRequired { .. } => "recent authentication required",
            Self::Unavailable => "factor storage unavailable",
        })
    }
}
impl std::error::Error for MfaError {}
impl From<sqlx::Error> for MfaError {
    fn from(_: sqlx::Error) -> Self {
        Self::Unavailable
    }
}
#[derive(Clone, Copy)]
pub struct MfaAudit {
    pub request_id: Uuid,
    pub source_hash: Digest,
}
pub struct Enrollment {
    pub challenge_id: Uuid,
    pub secret_base32: Zeroizing<String>,
    pub otpauth_uri: Zeroizing<String>,
    pub expires_at: OffsetDateTime,
}
pub struct EnrollmentConfirmInput {
    pub session_hash: Digest,
    pub challenge_id: Uuid,
    pub code: Zeroizing<String>,
    pub audit: MfaAudit,
}
#[derive(Clone, Copy)]
pub enum FactorMethod {
    Totp,
    Recovery,
}
pub struct LoginCompletion {
    pub session_id: Uuid,
    pub session_token_hash: Digest,
    pub session_csrf_hash: Digest,
    pub new_preauth_hash: Digest,
    pub new_preauth_csrf_hash: Digest,
    pub user_agent: String,
}
pub struct FactorVerificationInput {
    pub challenge_id: Uuid,
    pub code: Zeroizing<String>,
    pub method: FactorMethod,
    pub preauth_hash: Option<Digest>,
    pub session_hash: Option<Digest>,
    pub reauth_only: bool,
    pub login: LoginCompletion,
    pub audit: MfaAudit,
}
pub enum FactorOutcome {
    Authenticated {
        user: UserView,
        session: Box<SessionView>,
    },
    Reauthenticated {
        reauthenticated_at: OffsetDateTime,
        valid_until: OffsetDateTime,
        amr: Vec<String>,
    },
}
#[derive(Clone)]
pub struct MfaService {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    keys: Arc<AeadKeyRing>,
    issuer: String,
}
impl MfaService {
    pub fn new(
        pool: PgPool,
        clock: Arc<dyn Clock>,
        keys: Arc<AeadKeyRing>,
        issuer: String,
    ) -> Self {
        Self {
            pool,
            clock,
            keys,
            issuer,
        }
    }
    pub async fn enroll(&self, session: Digest, audit: MfaAudit) -> Result<Enrollment, MfaError> {
        let now = self.clock.now();
        let (mut tx, user, session_id) = self.lock_session(session, now).await?;
        let id: Uuid = user.try_get("id")?;
        require_recent(&mut tx, id, session_id, now).await?;
        if sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM totp_factors WHERE user_id=$1 AND confirmed)",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?
        {
            return Err(MfaError::AlreadyConfigured);
        }
        let secret = TotpSecret::generate().map_err(|_| MfaError::Unavailable)?;
        let encrypted = self
            .keys
            .encrypt(id, "totp-enrollment", secret.bytes())
            .map_err(|_| MfaError::Unavailable)?;
        let challenge_id = Uuid::new_v4();
        let expires_at = now + Duration::minutes(5);
        sqlx::query("SELECT id FROM authentication_challenges WHERE user_id=$1 AND session_id=$2 AND purpose='totp_enrollment' ORDER BY id FOR UPDATE").bind(id).bind(session_id).fetch_all(&mut *tx).await?;
        sqlx::query("UPDATE authentication_challenges SET consumed_at=COALESCE(consumed_at,$3) WHERE user_id=$1 AND session_id=$2 AND purpose='totp_enrollment'").bind(id).bind(session_id).bind(now).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO authentication_challenges(id,user_id,session_id,purpose,state_encrypted,state_data,created_at,expires_at) VALUES($1,$2,$3,'totp_enrollment',$4,$5,$6,$7)").bind(challenge_id).bind(id).bind(session_id).bind(sqlx::types::Json(encrypted)).bind(serde_json::json!({"credential_version":user.try_get::<i64,_>("credential_version")?})).bind(now).bind(expires_at).execute(&mut *tx).await?;
        let account: String = user.try_get("email")?;
        let result = Enrollment {
            challenge_id,
            secret_base32: secret.base32(),
            otpauth_uri: secret
                .otpauth_uri(&account)
                .map_err(|_| MfaError::Unavailable)?,
            expires_at,
        };
        let _ = &self.issuer;
        let _ = audit;
        tx.commit().await?;
        Ok(result)
    }
    pub async fn confirm_enrollment(
        &self,
        input: &EnrollmentConfirmInput,
    ) -> Result<RecoveryCodes, MfaError> {
        let now = self.clock.now();
        let (mut tx, user, session) = self.lock_session(input.session_hash, now).await?;
        let id: Uuid = user.try_get("id")?;
        require_recent(&mut tx, id, session, now).await?;
        let challenge=sqlx::query("SELECT * FROM authentication_challenges WHERE id=$1 AND user_id=$2 AND session_id=$3 AND purpose='totp_enrollment' FOR UPDATE").bind(input.challenge_id).bind(id).bind(session).fetch_optional(&mut *tx).await?.ok_or(MfaError::InvalidChallenge)?;
        validate_challenge(&challenge, now, user.try_get("credential_version")?)?;
        let envelope: AeadEnvelope =
            serde_json::from_value(challenge.try_get::<serde_json::Value, _>("state_encrypted")?)
                .map_err(|_| MfaError::InvalidChallenge)?;
        let bytes = self
            .keys
            .decrypt(id, "totp-enrollment", &envelope)
            .map_err(|_| MfaError::Unavailable)?;
        let secret = TotpSecret::from_bytes(&bytes).map_err(|_| MfaError::Unavailable)?;
        let Some(step) = secret.matching_step(&input.code, now.unix_timestamp()) else {
            return fail(tx, id, input.challenge_id, input.audit, now).await;
        };
        if sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM totp_factors WHERE user_id=$1 AND confirmed)",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?
        {
            return Err(MfaError::AlreadyConfigured);
        }
        let encrypted = self
            .keys
            .encrypt(id, "totp-seed", secret.bytes())
            .map_err(|_| MfaError::Unavailable)?;
        let ciphertext = BASE64_URL_SAFE_NO_PAD
            .decode(&encrypted.ciphertext)
            .map_err(|_| MfaError::Unavailable)?;
        let nonce = BASE64_URL_SAFE_NO_PAD
            .decode(&encrypted.nonce)
            .map_err(|_| MfaError::Unavailable)?;
        sqlx::query("INSERT INTO totp_factors(id,user_id,encrypted_seed,encryption_kid,encryption_nonce,confirmed,last_step,created_at) VALUES($1,$2,$3,$4,$5,true,$6,$7) ON CONFLICT(user_id) DO UPDATE SET encrypted_seed=EXCLUDED.encrypted_seed,encryption_kid=EXCLUDED.encryption_kid,encryption_nonce=EXCLUDED.encryption_nonce,confirmed=true,last_step=EXCLUDED.last_step,enrollment_expires_at=NULL").bind(Uuid::new_v4()).bind(id).bind(ciphertext).bind(&encrypted.kid).bind(nonce).bind(step).bind(now).execute(&mut *tx).await?;
        consume(&mut tx, input.challenge_id, now).await?;
        let codes = replace_recovery(&mut tx, id, now).await?;
        sqlx::query("UPDATE sessions SET strong_at=$2 WHERE id=$1")
            .bind(session)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        notification(&mut tx, &user, &self.keys, "mfa.totp_enrolled", now)
            .await
            .map_err(|_| MfaError::Unavailable)?;
        write_audit(&mut tx, id, AuditEvent::TotpEnabled, input.audit, now).await?;
        tx.commit().await?;
        Ok(codes)
    }
    pub async fn disable_totp(&self, hash: Digest, audit: MfaAudit) -> Result<(), MfaError> {
        let now = self.clock.now();
        let (mut tx, user, session) = self.lock_session_guarded(hash, now, true).await?;
        let id: Uuid = user.try_get("id")?;
        require_recent(&mut tx, id, session, now).await?;
        crate::admin::protect_factor_removal(&mut tx, id, true, None)
            .await
            .map_err(|error| {
                if error == crate::admin::AdminError::LastAdministrator {
                    MfaError::LastAdministrator
                } else {
                    MfaError::Unavailable
                }
            })?;
        sqlx::query("DELETE FROM totp_factors WHERE user_id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let has_passkey: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM webauthn_credentials WHERE user_id=$1)",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if !has_passkey {
            sqlx::query("DELETE FROM recovery_codes WHERE user_id=$1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        notification(&mut tx, &user, &self.keys, "mfa.totp_removed", now)
            .await
            .map_err(|_| MfaError::Unavailable)?;
        write_audit(&mut tx, id, AuditEvent::TotpDisabled, audit, now).await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn regenerate_recovery(
        &self,
        hash: Digest,
        audit: MfaAudit,
    ) -> Result<RecoveryCodes, MfaError> {
        let now = self.clock.now();
        let (mut tx, user, session) = self.lock_session(hash, now).await?;
        let id: Uuid = user.try_get("id")?;
        require_recent(&mut tx, id, session, now).await?;
        let has:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM totp_factors WHERE user_id=$1 AND confirmed) OR EXISTS(SELECT 1 FROM webauthn_credentials WHERE user_id=$1)").bind(id).fetch_one(&mut *tx).await?;
        if !has {
            return Err(MfaError::InvalidChallenge);
        }
        let codes = replace_recovery(&mut tx, id, now).await?;
        notification(
            &mut tx,
            &user,
            &self.keys,
            "mfa.recovery_codes_regenerated",
            now,
        )
        .await
        .map_err(|_| MfaError::Unavailable)?;
        write_audit(&mut tx, id, AuditEvent::RecoveryRegenerated, audit, now).await?;
        tx.commit().await?;
        Ok(codes)
    }
    pub async fn verify_factor(
        &self,
        input: &FactorVerificationInput,
    ) -> Result<FactorOutcome, MfaError> {
        let now = self.clock.now();
        let id: Option<Uuid> =
            sqlx::query_scalar("SELECT user_id FROM authentication_challenges WHERE id=$1")
                .bind(input.challenge_id)
                .fetch_optional(&self.pool)
                .await?
                .flatten();
        let id = id.ok_or(MfaError::InvalidChallenge)?;
        let mut tx = self.pool.begin().await?;
        let user=sqlx::query("SELECT id,email,display_name,verified,status,credential_version,created_at FROM users WHERE id=$1 FOR UPDATE").bind(id).fetch_one(&mut *tx).await?;
        if !user.try_get::<bool, _>("verified")? || user.try_get::<String, _>("status")? != "active"
        {
            return Err(MfaError::InvalidChallenge);
        }
        let located=sqlx::query("SELECT purpose,session_id,preauth_hash FROM authentication_challenges WHERE id=$1 AND user_id=$2").bind(input.challenge_id).bind(id).fetch_one(&mut *tx).await?;
        let purpose: String = located.try_get("purpose")?;
        if input.reauth_only && purpose != "reauthentication" {
            return Err(MfaError::InvalidChallenge);
        }
        let session_id: Option<Uuid> = located.try_get("session_id")?;
        if purpose == "reauthentication" {
            let hash = input.session_hash.ok_or(MfaError::InvalidSession)?;
            let session=sqlx::query("SELECT id FROM sessions WHERE token_hash=$1 AND user_id=$2 AND revoked_at IS NULL AND expires_at>$3 AND credential_version=$4 FOR UPDATE").bind(hash.as_bytes()).bind(id).bind(now).bind(user.try_get::<i64,_>("credential_version")?).fetch_optional(&mut *tx).await?.ok_or(MfaError::InvalidSession)?;
            if Some(session.try_get::<Uuid, _>("id")?) != session_id {
                return Err(MfaError::InvalidChallenge);
            }
        } else if purpose == "login" {
            let hash = input.preauth_hash.ok_or(MfaError::InvalidChallenge)?;
            let bound: Option<Vec<u8>> = located.try_get("preauth_hash")?;
            if bound.as_deref() != Some(hash.as_bytes()) {
                return Err(MfaError::InvalidChallenge);
            }
            let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM preauthentication_contexts WHERE token_hash=$1 AND revoked_at IS NULL AND expires_at>$2)").bind(hash.as_bytes()).bind(now).fetch_one(&mut *tx).await?;
            if !exists {
                return Err(MfaError::InvalidChallenge);
            }
        } else {
            return Err(MfaError::InvalidChallenge);
        }
        let challenge =
            sqlx::query("SELECT * FROM authentication_challenges WHERE id=$1 FOR UPDATE")
                .bind(input.challenge_id)
                .fetch_one(&mut *tx)
                .await?;
        validate_challenge(&challenge, now, user.try_get("credential_version")?)?;
        let amr = match input.method {
            FactorMethod::Totp => {
                let factor=sqlx::query("SELECT encrypted_seed,encryption_kid,encryption_nonce,last_step FROM totp_factors WHERE user_id=$1 AND confirmed FOR UPDATE").bind(id).fetch_optional(&mut *tx).await?;
                let Some(factor) = factor else {
                    return fail(tx, id, input.challenge_id, input.audit, now).await;
                };
                let envelope = AeadEnvelope {
                    kid: factor.try_get("encryption_kid")?,
                    nonce: BASE64_URL_SAFE_NO_PAD
                        .encode(factor.try_get::<Vec<u8>, _>("encryption_nonce")?),
                    ciphertext: BASE64_URL_SAFE_NO_PAD
                        .encode(factor.try_get::<Vec<u8>, _>("encrypted_seed")?),
                };
                let seed = self
                    .keys
                    .decrypt(id, "totp-seed", &envelope)
                    .map_err(|_| MfaError::Unavailable)?;
                let secret = TotpSecret::from_bytes(&seed).map_err(|_| MfaError::Unavailable)?;
                let Some(step) = secret.matching_step(&input.code, now.unix_timestamp()) else {
                    return fail(tx, id, input.challenge_id, input.audit, now).await;
                };
                if step <= factor.try_get::<i64, _>("last_step")? {
                    return fail(tx, id, input.challenge_id, input.audit, now).await;
                }
                sqlx::query("UPDATE totp_factors SET last_step=$2 WHERE user_id=$1")
                    .bind(id)
                    .bind(step)
                    .execute(&mut *tx)
                    .await?;
                vec!["pwd".into(), "otp".into()]
            }
            FactorMethod::Recovery => {
                let result=sqlx::query("UPDATE recovery_codes SET consumed_at=$3 WHERE user_id=$1 AND code_hash=$2 AND consumed_at IS NULL").bind(id).bind(token_digest(&input.code).as_slice()).bind(now).execute(&mut *tx).await?;
                if result.rows_affected() != 1 {
                    return fail(tx, id, input.challenge_id, input.audit, now).await;
                }
                write_audit(&mut tx, id, AuditEvent::RecoveryUsed, input.audit, now).await?;
                vec!["pwd".into(), "rcv".into()]
            }
        };
        consume(&mut tx, input.challenge_id, now).await?;
        if purpose == "reauthentication" {
            sqlx::query("UPDATE sessions SET strong_at=$2 WHERE id=$1")
                .bind(session_id)
                .bind(now)
                .execute(&mut *tx)
                .await?;
            write_audit(&mut tx, id, AuditEvent::Reauthenticated, input.audit, now).await?;
            tx.commit().await?;
            return Ok(FactorOutcome::Reauthenticated {
                reauthenticated_at: now,
                valid_until: now + Duration::minutes(5),
                amr,
            });
        }
        if input.login.user_agent.chars().count() > 256
            || input.login.user_agent.chars().any(char::is_control)
        {
            return Err(MfaError::InvalidChallenge);
        }
        let expires_at = now + Duration::hours(12);
        let hash = input.preauth_hash.ok_or(MfaError::InvalidChallenge)?;
        sqlx::query("INSERT INTO sessions(id,token_hash,user_id,amr,auth_time,strong_at,csrf_hash,credential_version,user_agent,created_at,expires_at) VALUES($1,$2,$3,$4,$5,$5,$6,$7,$8,$5,$9)").bind(input.login.session_id).bind(input.login.session_token_hash.as_bytes()).bind(id).bind(&amr).bind(now).bind(input.login.session_csrf_hash.as_bytes()).bind(user.try_get::<i64,_>("credential_version")?).bind(&input.login.user_agent).bind(expires_at).execute(&mut *tx).await?;
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
        .map_err(|_| MfaError::Unavailable)?;
        write_audit(&mut tx, id, AuditEvent::LoginSucceeded, input.audit, now).await?;
        let view = user_view(&user).map_err(|_| MfaError::Unavailable)?;
        tx.commit().await?;
        Ok(FactorOutcome::Authenticated {
            user: view,
            session: Box::new(SessionView {
                id: input.login.session_id,
                amr,
                auth_time: now,
                strong_at: Some(now),
                expires_at,
                created_at: now,
                user_agent: input.login.user_agent.clone(),
                current: true,
            }),
        })
    }
    async fn lock_session(
        &self,
        hash: Digest,
        now: OffsetDateTime,
    ) -> Result<(Transaction<'_, Postgres>, PgRow, Uuid), MfaError> {
        self.lock_session_guarded(hash, now, false).await
    }
    async fn lock_session_guarded(
        &self,
        hash: Digest,
        now: OffsetDateTime,
        admin_guard: bool,
    ) -> Result<(Transaction<'_, Postgres>, PgRow, Uuid), MfaError> {
        let id: Option<Uuid> =
            sqlx::query_scalar("SELECT user_id FROM sessions WHERE token_hash=$1")
                .bind(hash.as_bytes())
                .fetch_optional(&self.pool)
                .await?;
        let id = id.ok_or(MfaError::InvalidSession)?;
        let mut tx = self.pool.begin().await?;
        if admin_guard {
            crate::admin::guard(&mut tx)
                .await
                .map_err(|_| MfaError::Unavailable)?;
        }
        let user=sqlx::query("SELECT id,email,verified,status,credential_version,created_at FROM users WHERE id=$1 FOR UPDATE").bind(id).fetch_one(&mut *tx).await?;
        if !user.try_get::<bool, _>("verified")? || user.try_get::<String, _>("status")? != "active"
        {
            return Err(MfaError::InvalidSession);
        }
        let session:Option<Uuid>=sqlx::query_scalar("SELECT id FROM sessions WHERE token_hash=$1 AND user_id=$2 AND credential_version=$3 AND revoked_at IS NULL AND expires_at>$4 FOR UPDATE").bind(hash.as_bytes()).bind(id).bind(user.try_get::<i64,_>("credential_version")?).bind(now).fetch_optional(&mut *tx).await?;
        Ok((tx, user, session.ok_or(MfaError::InvalidSession)?))
    }
}
fn validate_challenge(row: &PgRow, now: OffsetDateTime, version: i64) -> Result<(), MfaError> {
    if row
        .try_get::<Option<OffsetDateTime>, _>("consumed_at")?
        .is_some()
    {
        return Err(MfaError::ConsumedChallenge);
    }
    if row.try_get::<OffsetDateTime, _>("expires_at")? <= now {
        return Err(MfaError::ExpiredChallenge);
    }
    if row.try_get::<i32, _>("attempts")? >= 5 {
        return Err(MfaError::AttemptsExhausted);
    }
    let state: serde_json::Value = row.try_get("state_data")?;
    if state
        .get("credential_version")
        .and_then(serde_json::Value::as_i64)
        != Some(version)
    {
        return Err(MfaError::InvalidChallenge);
    }
    Ok(())
}
pub(crate) async fn require_recent(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
    session: Uuid,
    now: OffsetDateTime,
) -> Result<(), MfaError> {
    let row=sqlx::query("SELECT s.password_confirmed_at,s.strong_at,EXISTS(SELECT 1 FROM totp_factors f WHERE f.user_id=$1 AND f.confirmed) OR EXISTS(SELECT 1 FROM webauthn_credentials k WHERE k.user_id=$1) AS has_mfa FROM sessions s WHERE id=$2").bind(user).bind(session).fetch_one(&mut **tx).await?;
    let strong: bool = row.try_get("has_mfa")?;
    let at: Option<OffsetDateTime> = row.try_get(if strong {
        "strong_at"
    } else {
        "password_confirmed_at"
    })?;
    if at.is_none_or(|at| at > now || at + Duration::minutes(5) <= now) {
        return Err(MfaError::ReauthRequired {
            strong,
            methods: if strong {
                factor_methods(tx, user)
                    .await
                    .map_err(|_| MfaError::Unavailable)?
            } else {
                vec!["password".into()]
            },
        });
    }
    Ok(())
}
async fn consume(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    now: OffsetDateTime,
) -> Result<(), MfaError> {
    sqlx::query(
        "UPDATE authentication_challenges SET consumed_at=$2,state_encrypted=NULL WHERE id=$1",
    )
    .bind(id)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
async fn replace_recovery(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
    now: OffsetDateTime,
) -> Result<RecoveryCodes, MfaError> {
    let codes = RecoveryCodes::generate().map_err(|_| MfaError::Unavailable)?;
    sqlx::query("DELETE FROM recovery_codes WHERE user_id=$1")
        .bind(user)
        .execute(&mut **tx)
        .await?;
    for digest in codes.digests() {
        sqlx::query(
            "INSERT INTO recovery_codes(id,user_id,code_hash,created_at) VALUES($1,$2,$3,$4)",
        )
        .bind(Uuid::new_v4())
        .bind(user)
        .bind(digest.as_slice())
        .bind(now)
        .execute(&mut **tx)
        .await?;
    }
    Ok(codes)
}
async fn fail<T>(
    mut tx: Transaction<'_, Postgres>,
    user: Uuid,
    challenge: Uuid,
    audit: MfaAudit,
    now: OffsetDateTime,
) -> Result<T, MfaError> {
    let attempts:i32=sqlx::query_scalar("UPDATE authentication_challenges SET attempts=attempts+1,consumed_at=CASE WHEN attempts+1>=5 THEN $2 ELSE NULL END,state_encrypted=CASE WHEN attempts+1>=5 THEN NULL ELSE state_encrypted END WHERE id=$1 RETURNING attempts").bind(challenge).bind(now).fetch_one(&mut *tx).await?;
    write_audit(
        &mut tx,
        user,
        if attempts >= 5 {
            AuditEvent::ChallengeExhausted
        } else {
            AuditEvent::ChallengeFailed
        },
        audit,
        now,
    )
    .await?;
    tx.commit().await?;
    Err(if attempts >= 5 {
        MfaError::AttemptsExhausted
    } else {
        MfaError::InvalidCode
    })
}
async fn write_audit(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
    event: AuditEvent,
    audit: MfaAudit,
    now: OffsetDateTime,
) -> Result<(), MfaError> {
    insert_audit(
        tx,
        &AuditRecord {
            id: Uuid::new_v4(),
            event,
            actor_id: Some(user),
            target: AuditTarget::User,
            target_id: Some(user),
            result: if matches!(
                event,
                AuditEvent::ChallengeFailed | AuditEvent::ChallengeExhausted
            ) {
                AuditResult::Failure
            } else {
                AuditResult::Success
            },
            request_id: audit.request_id,
            source_hash: audit.source_hash,
            occurred_at: now,
        },
    )
    .await
    .map_err(|_| MfaError::Unavailable)
}
