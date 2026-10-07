//! Durable CSRF/preauthentication and transaction-bound sanitized audit writes.
use crate::repository::{Digest, RepositoryError};
use sqlx::{PgPool, Postgres, Transaction};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

#[derive(Clone)]
pub struct BrowserSecurityStore {
    pool: PgPool,
}
impl BrowserSecurityStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Checks the actual current user/session state; preauthentication never grants identity.
    pub async fn valid_session(
        &self,
        session: Digest,
        now: OffsetDateTime,
    ) -> Result<bool, RepositoryError> {
        Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=$1 AND s.revoked_at IS NULL AND s.expires_at>$2 AND s.credential_version=u.credential_version AND u.verified AND u.status='active')")
            .bind(session.as_bytes()).bind(now).fetch_one(&self.pool).await?)
    }

    pub async fn rotate_session_csrf(
        &self,
        session: Digest,
        csrf: Digest,
        now: OffsetDateTime,
    ) -> Result<bool, RepositoryError> {
        let rows=sqlx::query("UPDATE sessions s SET csrf_hash=$2 FROM users u WHERE s.token_hash=$1 AND u.id=s.user_id AND s.revoked_at IS NULL AND s.expires_at>$3 AND s.credential_version=u.credential_version AND u.verified AND u.status='active'")
            .bind(session.as_bytes()).bind(csrf.as_bytes()).bind(now).execute(&self.pool).await?.rows_affected();
        Ok(rows == 1)
    }

    /// A CSRF refresh preserves the same context/absolute expiry, including bound OAuth transactions.
    pub async fn rotate_preauth_csrf(
        &self,
        token: Digest,
        csrf: Digest,
        now: OffsetDateTime,
    ) -> Result<bool, RepositoryError> {
        let count=sqlx::query("UPDATE preauthentication_contexts SET csrf_hash=$2 WHERE token_hash=$1 AND revoked_at IS NULL AND expires_at>$3")
            .bind(token.as_bytes()).bind(csrf.as_bytes()).bind(now).execute(&self.pool).await?.rows_affected();
        Ok(count == 1)
    }

    /// Replacement revokes the old context in the same commit. No plaintext cookie/CSRF enters SQL.
    pub async fn replace_preauth(
        &self,
        old: Option<Digest>,
        token: Digest,
        csrf: Digest,
        now: OffsetDateTime,
    ) -> Result<(), RepositoryError> {
        let mut tx = self.pool.begin().await?;
        if let Some(old) = old {
            sqlx::query("UPDATE preauthentication_contexts SET revoked_at=COALESCE(revoked_at,$2) WHERE token_hash=$1")
                .bind(old.as_bytes()).bind(now).execute(&mut *tx).await?;
        }
        sqlx::query("INSERT INTO preauthentication_contexts(id,token_hash,csrf_hash,created_at,expires_at) VALUES($1,$2,$3,$4,$5)")
            .bind(Uuid::new_v4()).bind(token.as_bytes()).bind(csrf.as_bytes()).bind(now).bind(now+Duration::minutes(10)).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn valid_csrf(
        &self,
        session: Option<Digest>,
        preauth: Option<Digest>,
        csrf: Digest,
        now: OffsetDateTime,
    ) -> Result<bool, RepositoryError> {
        if let Some(session) = session {
            // A supplied session is authoritative; do not accept a different preauth CSRF to bypass it.
            return Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=$1 AND s.csrf_hash=$2 AND s.revoked_at IS NULL AND s.expires_at>$3 AND s.credential_version=u.credential_version AND u.verified AND u.status='active')")
                .bind(session.as_bytes()).bind(csrf.as_bytes()).bind(now).fetch_one(&self.pool).await?);
        }
        if let Some(preauth) = preauth {
            return Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM preauthentication_contexts WHERE token_hash=$1 AND csrf_hash=$2 AND revoked_at IS NULL AND expires_at>$3)")
                .bind(preauth.as_bytes()).bind(csrf.as_bytes()).bind(now).fetch_one(&self.pool).await?);
        }
        Ok(false)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum AuditEvent {
    LoginSucceeded,
    LoginFailed,
    EmailVerified,
    PasswordReset,
    PasswordChanged,
    Reauthenticated,
    TotpEnabled,
    TotpDisabled,
    RecoveryUsed,
    RecoveryRegenerated,
    PasskeyRegistered,
    PasskeyRenamed,
    PasskeyDeleted,
    SessionRevoked,
    SessionsRevoked,
    ConsentApproved,
    ConsentDenied,
    TokenRefreshed,
    RefreshReuseDetected,
    GrantRevoked,
    UserStatusChanged,
    RegistrationRequested,
    EmailVerificationRequested,
    ChallengeFailed,
    ChallengeExhausted,
    PasswordResetRequested,
    CodeConsumed,
    ClientDisabled,
    AdminInitialized,
    LastAdminRemovalDenied,
    UserEnabled,
    RateLimited,
    DependencyUnavailable,
    EmailDeliveryFailed,
    EmailDeliverySucceeded,
    SigningKeyRotated,
    EncryptionKeyRotated,
    UserSessionsRevoked,
    ClientCreated,
    ClientUpdated,
    ClientSecretRotated,
    AdminAdded,
    AdminRemoved,
}
impl AuditEvent {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LoginSucceeded => "auth.session_created",
            Self::LoginFailed => "auth.password_failed",
            Self::EmailVerified => "email.verified",
            Self::PasswordReset => "password.reset_completed",
            Self::PasswordChanged => "password.changed",
            Self::Reauthenticated => "auth.reauthenticated",
            Self::TotpEnabled => "mfa.totp_enrolled",
            Self::TotpDisabled => "mfa.totp_removed",
            Self::RecoveryUsed => "mfa.recovery_code_used",
            Self::RecoveryRegenerated => "mfa.recovery_codes_regenerated",
            Self::PasskeyRegistered => "passkey.registered",
            Self::PasskeyRenamed => "passkey.renamed",
            Self::PasskeyDeleted => "passkey.removed",
            Self::SessionRevoked => "auth.session_revoked",
            Self::SessionsRevoked => "auth.sessions_revoked_all",
            Self::ConsentApproved => "oauth.consent_granted",
            Self::ConsentDenied => "oauth.consent_denied",
            Self::TokenRefreshed => "oauth.refresh_rotated",
            Self::RefreshReuseDetected => "oauth.refresh_replay_detected",
            Self::GrantRevoked => "oauth.grant_revoked",
            Self::UserStatusChanged => "admin.user_disabled",
            Self::UserSessionsRevoked => "admin.user_sessions_revoked",
            Self::ClientCreated => "oauth.client_created",
            Self::ClientUpdated => "oauth.client_updated",
            Self::ClientSecretRotated => "oauth.client_secret_rotated",
            Self::AdminAdded => "admin.member_granted",
            Self::AdminRemoved => "admin.member_removed",
            Self::RegistrationRequested => "identity.registration_requested",
            Self::EmailVerificationRequested => "email.verification_requested",
            Self::ChallengeFailed => "auth.challenge_failed",
            Self::ChallengeExhausted => "auth.challenge_exhausted",
            Self::PasswordResetRequested => "password.reset_requested",
            Self::CodeConsumed => "oauth.code_consumed",
            Self::ClientDisabled => "oauth.client_disabled",
            Self::AdminInitialized => "admin.initialized",
            Self::LastAdminRemovalDenied => "admin.last_member_removal_denied",
            Self::UserEnabled => "admin.user_enabled",
            Self::RateLimited => "system.rate_limited",
            Self::DependencyUnavailable => "system.dependency_unavailable",
            Self::EmailDeliveryFailed => "email.delivery_failed",
            Self::EmailDeliverySucceeded => "email.delivery_succeeded",
            Self::SigningKeyRotated => "keys.signing_rotated",
            Self::EncryptionKeyRotated => "keys.encryption_rotated",
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub enum AuditResult {
    Success,
    Denied,
    Failure,
}
impl AuditResult {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Denied => "denied",
            Self::Failure => "failure",
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub enum AuditTarget {
    User,
    Session,
    Grant,
    Client,
    AdminMember,
    Credential,
    Challenge,
}
impl AuditTarget {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Session => "session",
            Self::Grant => "grant",
            Self::Client => "client",
            Self::AdminMember => "admin_member",
            Self::Credential => "credential",
            Self::Challenge => "challenge",
        }
    }
}

pub struct AuditRecord {
    pub id: Uuid,
    pub event: AuditEvent,
    pub actor_id: Option<Uuid>,
    pub target: AuditTarget,
    pub target_id: Option<Uuid>,
    pub result: AuditResult,
    pub request_id: Uuid,
    pub source_hash: Digest,
    pub occurred_at: OffsetDateTime,
}

/// Must be called with the same transaction as the security change; failure prevents its commit.
pub async fn insert_audit(
    transaction: &mut Transaction<'_, Postgres>,
    record: &AuditRecord,
) -> Result<(), RepositoryError> {
    let source = record
        .source_hash
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    sqlx::query("INSERT INTO audit_events(id,event,actor_id,target_type,target_id,result,request_id,source,occurred_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(record.id).bind(record.event.as_str()).bind(record.actor_id).bind(record.target.as_str()).bind(record.target_id).bind(record.result.as_str()).bind(record.request_id).bind(source).bind(record.occurred_at).execute(&mut **transaction).await?;
    Ok(())
}
