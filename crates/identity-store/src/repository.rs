//! Parameterized authority queries and ordered security transactions.
//! Factor/password/JOSE proofs belong to later service modules; these primitives never authenticate by themselves.
use identity_core::clock::Clock;
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use std::{fmt, sync::Arc};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

/// SHA-256/HMAC-sized stored digest. Raw tokens cannot be passed to digest columns.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Digest([u8; 32]);

impl Digest {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
    pub fn from_slice(bytes: &[u8]) -> Result<Self, RepositoryError> {
        bytes
            .try_into()
            .map(Self)
            .map_err(|_| RepositoryError::InvalidState)
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Digest([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepositoryError {
    NotFound,
    Conflict,
    Replayed,
    InvalidState,
    Inactive,
    Unavailable,
}

impl fmt::Display for RepositoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotFound => "repository resource not found",
            Self::Conflict => "repository state conflict",
            Self::Replayed => "repository token already consumed",
            Self::InvalidState => "repository state invalid",
            Self::Inactive => "repository user inactive",
            Self::Unavailable => "repository unavailable",
        })
    }
}
impl std::error::Error for RepositoryError {}
impl From<sqlx::Error> for RepositoryError {
    fn from(error: sqlx::Error) -> Self {
        match error {
            sqlx::Error::RowNotFound => Self::NotFound,
            sqlx::Error::Database(ref e) if e.code().as_deref() == Some("23505") => Self::Conflict,
            sqlx::Error::Database(ref e)
                if matches!(e.code().as_deref(), Some("23503" | "23514" | "23502")) =>
            {
                Self::InvalidState
            }
            _ => Self::Unavailable,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UserRecord {
    pub id: Uuid,
    pub email: String,
    pub verified: bool,
    pub status: String,
    pub credential_version: i64,
    pub created_at: OffsetDateTime,
}
fn user_record(row: &PgRow) -> Result<UserRecord, RepositoryError> {
    Ok(UserRecord {
        id: row.try_get("id")?,
        email: row.try_get("email")?,
        verified: row.try_get("verified")?,
        status: row.try_get("status")?,
        credential_version: row.try_get("credential_version")?,
        created_at: row.try_get("created_at")?,
    })
}

#[derive(Debug)]
pub struct SessionAuthority {
    pub user_id: Uuid,
    pub session_id: Uuid,
    pub credential_version: i64,
    pub auth_time: OffsetDateTime,
    pub strong_at: Option<OffsetDateTime>,
    pub expires_at: OffsetDateTime,
}

#[derive(Debug)]
pub struct TokenAuthority {
    pub user_id: Uuid,
    pub session_id: Uuid,
    pub grant_id: Uuid,
    pub token_id: Uuid,
    pub client_id: String,
    pub scope: Vec<String>,
    pub expires_at: OffsetDateTime,
    pub issued_at: OffsetDateTime,
}

#[derive(Clone)]
pub struct Repository {
    pool: PgPool,
    clock: Arc<dyn Clock>,
}
impl Repository {
    pub fn new(pool: PgPool, clock: Arc<dyn Clock>) -> Self {
        Self { pool, clock }
    }

    /// Read-only credential lookup; no authentication or token issuance is implied.
    pub async fn user_by_email(
        &self,
        normalized_email: &str,
    ) -> Result<Option<UserRecord>, RepositoryError> {
        let row = sqlx::query("SELECT id,email,verified,status,credential_version,created_at FROM users WHERE email=$1")
            .bind(normalized_email).fetch_optional(&self.pool).await?;
        row.as_ref().map(user_record).transpose()
    }

    /// Checks database authority each time, including the current credential version.
    pub async fn session_authority(
        &self,
        digest: Digest,
    ) -> Result<Option<SessionAuthority>, RepositoryError> {
        let row = sqlx::query("SELECT s.id AS session_id,u.id AS user_id,u.credential_version,s.auth_time,s.strong_at,s.expires_at FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=$1 AND s.revoked_at IS NULL AND s.expires_at>$2 AND u.verified AND u.status='active' AND s.credential_version=u.credential_version")
            .bind(digest.as_bytes()).bind(self.clock.now()).fetch_optional(&self.pool).await?;
        row.map(|r| {
            Ok(SessionAuthority {
                user_id: r.try_get("user_id")?,
                session_id: r.try_get("session_id")?,
                credential_version: r.try_get("credential_version")?,
                auth_time: r.try_get("auth_time")?,
                strong_at: r.try_get("strong_at")?,
                expires_at: r.try_get("expires_at")?,
            })
        })
        .transpose()
    }

    /// No Redis/positive cache: verifies user/session/grant/client/token in one current snapshot.
    pub async fn token_authority(
        &self,
        digest: Digest,
        client_id: &str,
    ) -> Result<Option<TokenAuthority>, RepositoryError> {
        let row = sqlx::query("SELECT t.id AS token_id,u.id AS user_id,s.id AS session_id,g.id AS grant_id,c.client_id,g.scopes AS scope,LEAST(t.expires_at,g.expires_at,s.expires_at) AS expires_at,t.created_at AS issued_at FROM oauth_tokens t JOIN oauth_grants g ON g.id=t.grant_id JOIN sessions s ON s.id=g.session_id JOIN users u ON u.id=g.user_id AND u.id=s.user_id JOIN oauth_clients c ON c.id=g.client_id WHERE t.token_hash=$1 AND c.client_id=$2 AND t.revoked_at IS NULL AND t.consumed_at IS NULL AND t.expires_at>$3 AND g.revoked_at IS NULL AND g.expires_at>$3 AND s.revoked_at IS NULL AND s.expires_at>$3 AND s.credential_version=u.credential_version AND u.verified AND u.status='active' AND c.enabled")
            .bind(digest.as_bytes()).bind(client_id).bind(self.clock.now()).fetch_optional(&self.pool).await?;
        row.map(|r| {
            Ok(TokenAuthority {
                user_id: r.try_get("user_id")?,
                session_id: r.try_get("session_id")?,
                grant_id: r.try_get("grant_id")?,
                token_id: r.try_get("token_id")?,
                client_id: r.try_get("client_id")?,
                scope: r.try_get("scope")?,
                expires_at: r.try_get("expires_at")?,
                issued_at: r.try_get("issued_at")?,
            })
        })
        .transpose()
    }

    /// Locks user first. Subsequent locks are only acquired through the ordered wrapper.
    pub async fn begin_security(
        &self,
        user_id: Uuid,
    ) -> Result<SecurityTransaction<'_>, RepositoryError> {
        let mut transaction = self.pool.begin().await?;
        let row=sqlx::query("SELECT id,email,verified,status,credential_version,created_at FROM users WHERE id=$1 FOR UPDATE")
            .bind(user_id).fetch_one(&mut *transaction).await?;
        let user = user_record(&row)?;
        Ok(SecurityTransaction {
            transaction,
            user,
            clock: self.clock.clone(),
            phase: LockPhase::User,
            session_id: None,
            grant_id: None,
            failed: false,
            login_consumed: false,
            login_session_created: false,
            locked_code_id: None,
            locked_family_id: None,
            locked_family_expires_at: None,
            token_issue_permitted: false,
            token_inserted: false,
            issued_family_id: None,
            inserted_access: false,
            inserted_refresh: false,
            consumed_verify_action: false,
            verification_applied: false,
        })
    }

    /// Outbox leases have their own transaction: no SMTP is performed under database locks.
    pub async fn claim_outbox(
        &self,
        owner: Uuid,
        limit: u32,
        lease: Duration,
    ) -> Result<Vec<OutboxLease>, RepositoryError> {
        if limit == 0 || limit > 100 || lease <= Duration::ZERO || lease > Duration::minutes(10) {
            return Err(RepositoryError::InvalidState);
        }
        let now = self.clock.now();
        let lease_until = now + lease;
        // A fresh nonce for each claim prevents a prior claim by the same worker completing a new lease.
        let lease_id = Uuid::new_v4();
        let mut tx = self.pool.begin().await?;
        let rows=sqlx::query("WITH picked AS (SELECT id FROM email_outbox WHERE state='pending' AND next_attempt_at<=$1 AND (lease_until IS NULL OR lease_until<=$1) ORDER BY next_attempt_at,id FOR UPDATE SKIP LOCKED LIMIT $2) UPDATE email_outbox o SET lease_id=$3,lease_until=$4,attempts=o.attempts+1 FROM picked WHERE o.id=picked.id RETURNING o.id,o.user_id,o.recipient,o.template,o.encrypted_params,o.attempts,o.lease_until")
            .bind(now).bind(i64::from(limit)).bind(lease_id).bind(lease_until).fetch_all(&mut *tx).await?;
        let leases = rows
            .into_iter()
            .map(|r| {
                Ok(OutboxLease {
                    id: r.try_get("id")?,
                    user_id: r.try_get("user_id")?,
                    owner: lease_id,
                    worker_id: owner,
                    recipient: r.try_get("recipient")?,
                    template: r.try_get("template")?,
                    encrypted_params: r.try_get("encrypted_params")?,
                    attempts: r.try_get("attempts")?,
                    lease_until: r.try_get("lease_until")?,
                })
            })
            .collect::<Result<Vec<_>, RepositoryError>>()?;
        tx.commit().await?;
        Ok(leases)
    }

    /// Rejects stale lease completion. Once SMTP is real, acknowledged messages clear encrypted secrets.
    pub async fn complete_outbox(&self, id: Uuid, owner: Uuid) -> Result<(), RepositoryError> {
        let now = self.clock.now();
        let result=sqlx::query("UPDATE email_outbox SET state='delivered',delivered_at=$3,encrypted_params=NULL,lease_id=NULL,lease_until=NULL WHERE id=$1 AND lease_id=$2 AND lease_until>$3 AND state='pending'")
            .bind(id).bind(owner).bind(now).execute(&self.pool).await?;
        if result.rows_affected() != 1 {
            return Err(RepositoryError::Conflict);
        }
        Ok(())
    }
}

/// Never log this record: recipient and ciphertext are only passed to the worker service.
pub struct OutboxLease {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    /// The fresh per-claim lease nonce, required by complete_outbox.
    pub owner: Uuid,
    pub worker_id: Uuid,
    pub recipient: String,
    pub template: String,
    pub encrypted_params: sqlx::types::Json<serde_json::Value>,
    pub attempts: i32,
    pub lease_until: OffsetDateTime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum LockPhase {
    User,
    Session,
    Grant,
    Artifact,
}

/// The SQLx transaction is private. Services commit the entire action once, never a substep.
pub struct SecurityTransaction<'a> {
    transaction: Transaction<'a, Postgres>,
    user: UserRecord,
    clock: Arc<dyn Clock>,
    phase: LockPhase,
    session_id: Option<Uuid>,
    grant_id: Option<Uuid>,
    failed: bool,
    login_consumed: bool,
    login_session_created: bool,
    locked_code_id: Option<Uuid>,
    locked_family_id: Option<Uuid>,
    locked_family_expires_at: Option<OffsetDateTime>,
    token_issue_permitted: bool,
    token_inserted: bool,
    issued_family_id: Option<Uuid>,
    inserted_access: bool,
    inserted_refresh: bool,
    consumed_verify_action: bool,
    verification_applied: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum ChallengeBinding {
    Preauth(Digest),
    Session(Uuid),
}
#[derive(Clone, Copy, Debug)]
pub enum ChallengePurpose {
    Login,
    Reauthentication,
    TotpEnrollment,
    PasskeyRegistration,
    PasskeyLogin,
    PasskeyReauthentication,
}
impl ChallengePurpose {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Reauthentication => "reauthentication",
            Self::TotpEnrollment => "totp_enrollment",
            Self::PasskeyRegistration => "passkey_registration",
            Self::PasskeyLogin => "passkey_login",
            Self::PasskeyReauthentication => "passkey_reauthentication",
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub enum ActionPurpose {
    VerifyEmail,
    ResetPassword,
}
impl ActionPurpose {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VerifyEmail => "verify",
            Self::ResetPassword => "reset",
        }
    }
}

pub struct SessionInsert {
    pub id: Uuid,
    pub token_hash: Digest,
    pub csrf_hash: Digest,
    pub amr: Vec<String>,
    pub auth_time: OffsetDateTime,
    pub strong_at: Option<OffsetDateTime>,
    pub expires_at: OffsetDateTime,
    pub user_agent: Option<String>,
}

impl SecurityTransaction<'_> {
    pub fn user(&self) -> &UserRecord {
        &self.user
    }
    fn move_to(&mut self, phase: LockPhase) -> Result<(), RepositoryError> {
        if self.failed || phase < self.phase {
            self.failed = true;
            return Err(RepositoryError::InvalidState);
        }
        self.phase = phase;
        Ok(())
    }
    fn active_user(&mut self) -> Result<(), RepositoryError> {
        if self.failed || self.user.status != "active" || !self.user.verified {
            self.failed = true;
            return Err(RepositoryError::Inactive);
        }
        Ok(())
    }

    pub async fn lock_session(&mut self, id: Uuid) -> Result<SessionAuthority, RepositoryError> {
        if self.session_id.is_some() {
            self.failed = true;
            return Err(RepositoryError::InvalidState);
        }
        self.move_to(LockPhase::Session)?;
        self.active_user()?;
        let row=sqlx::query("SELECT id,auth_time,strong_at,expires_at FROM sessions WHERE id=$1 AND user_id=$2 AND revoked_at IS NULL AND expires_at>$3 AND credential_version=$4 FOR UPDATE")
            .bind(id).bind(self.user.id).bind(self.clock.now()).bind(self.user.credential_version).fetch_one(&mut *self.transaction).await?;
        self.session_id = Some(id);
        Ok(SessionAuthority {
            user_id: self.user.id,
            session_id: id,
            credential_version: self.user.credential_version,
            auth_time: row.try_get("auth_time")?,
            strong_at: row.try_get("strong_at")?,
            expires_at: row.try_get("expires_at")?,
        })
    }

    pub async fn lock_grant(&mut self, id: Uuid) -> Result<(), RepositoryError> {
        if self.grant_id.is_some() || self.session_id.is_none() {
            self.failed = true;
            return Err(RepositoryError::InvalidState);
        }
        self.move_to(LockPhase::Grant)?;
        let row=sqlx::query("SELECT g.id FROM oauth_grants g JOIN oauth_clients c ON c.id=g.client_id WHERE g.id=$1 AND g.user_id=$2 AND g.session_id=$3 AND g.revoked_at IS NULL AND g.expires_at>$4 AND c.enabled FOR UPDATE OF g")
            .bind(id).bind(self.user.id).bind(self.session_id).bind(self.clock.now()).fetch_optional(&mut *self.transaction).await?;
        if row.is_none() {
            return Err(RepositoryError::NotFound);
        }
        self.grant_id = Some(id);
        Ok(())
    }

    /// Inserts only persisted authority. Caller service must have verified the actual password/factor proof.
    pub async fn insert_session(&mut self, input: &SessionInsert) -> Result<(), RepositoryError> {
        self.active_user()?;
        let now = self.clock.now();
        if (self.login_consumed && self.login_session_created)
            || input.expires_at <= now
            || input.expires_at > input.auth_time + Duration::hours(12)
            || input.auth_time > now
            || input.amr.is_empty()
            || input
                .amr
                .iter()
                .any(|method| !matches!(method.as_str(), "pwd" | "otp" | "rcv" | "user" | "hwk"))
            || input.strong_at.is_some_and(|strong| {
                strong < input.auth_time || strong > now || strong >= input.expires_at
            })
            || input
                .user_agent
                .as_ref()
                .is_some_and(|agent| agent.chars().count() > 256)
        {
            self.failed = true;
            return Err(RepositoryError::InvalidState);
        }
        sqlx::query("INSERT INTO sessions(id,token_hash,user_id,amr,auth_time,strong_at,csrf_hash,expires_at,credential_version,user_agent,created_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
            .bind(input.id).bind(input.token_hash.as_bytes()).bind(self.user.id).bind(&input.amr).bind(input.auth_time).bind(input.strong_at).bind(input.csrf_hash.as_bytes()).bind(input.expires_at).bind(self.user.credential_version).bind(input.user_agent.as_deref().unwrap_or("")).bind(now).execute(&mut *self.transaction).await?;
        if self.login_consumed {
            self.login_session_created = true;
        }
        Ok(())
    }

    pub async fn consume_challenge(
        &mut self,
        id: Uuid,
        purpose: ChallengePurpose,
        binding: ChallengeBinding,
    ) -> Result<(), RepositoryError> {
        if self.login_consumed
            && matches!(
                purpose,
                ChallengePurpose::Login | ChallengePurpose::PasskeyLogin
            )
        {
            self.failed = true;
            return Err(RepositoryError::InvalidState);
        }
        match binding {
            ChallengeBinding::Session(id) if self.session_id != Some(id) => {
                self.failed = true;
                return Err(RepositoryError::InvalidState);
            }
            _ => {}
        }
        if matches!(
            purpose,
            ChallengePurpose::Reauthentication
                | ChallengePurpose::TotpEnrollment
                | ChallengePurpose::PasskeyRegistration
                | ChallengePurpose::PasskeyReauthentication
        ) && !matches!(binding, ChallengeBinding::Session(_))
        {
            self.failed = true;
            return Err(RepositoryError::InvalidState);
        }
        if matches!(
            purpose,
            ChallengePurpose::Login | ChallengePurpose::PasskeyLogin
        ) && !matches!(binding, ChallengeBinding::Preauth(_))
        {
            self.failed = true;
            return Err(RepositoryError::InvalidState);
        }
        self.move_to(LockPhase::Artifact)?;
        let (preauth, session) = match binding {
            ChallengeBinding::Preauth(d) => (Some(d), None),
            ChallengeBinding::Session(id) => (None, Some(id)),
        };
        let result=sqlx::query("UPDATE authentication_challenges SET consumed_at=$5 WHERE id=$1 AND purpose=$2 AND (user_id=$3 OR (user_id IS NULL AND purpose='passkey_login')) AND consumed_at IS NULL AND expires_at>$5 AND attempts<5 AND (($4::bytea IS NOT NULL AND preauth_hash=$4 AND session_id IS NULL) OR ($6::uuid IS NOT NULL AND session_id=$6 AND preauth_hash IS NULL))")
            .bind(id).bind(purpose.as_str()).bind(self.user.id).bind(preauth.as_ref().map(Digest::as_bytes)).bind(self.clock.now()).bind(session).execute(&mut *self.transaction).await?;
        if result.rows_affected() != 1 {
            self.failed = true;
            return Err(RepositoryError::Conflict);
        }
        if matches!(
            purpose,
            ChallengePurpose::Login | ChallengePurpose::PasskeyLogin
        ) {
            self.login_consumed = true;
        }
        Ok(())
    }

    /// Consumption and issued session stay in the same uncommitted transaction.
    pub async fn consume_login_and_create_session(
        &mut self,
        id: Uuid,
        purpose: ChallengePurpose,
        preauth: Digest,
        input: &SessionInsert,
    ) -> Result<(), RepositoryError> {
        if !matches!(
            purpose,
            ChallengePurpose::Login | ChallengePurpose::PasskeyLogin
        ) {
            return Err(RepositoryError::InvalidState);
        }
        self.active_user()?;
        self.consume_challenge(id, purpose, ChallengeBinding::Preauth(preauth))
            .await?;
        let result = self.insert_session(input).await;
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    pub async fn consume_email_action(
        &mut self,
        digest: Digest,
        purpose: ActionPurpose,
    ) -> Result<Uuid, RepositoryError> {
        if self.consumed_verify_action {
            self.failed = true;
            return Err(RepositoryError::InvalidState);
        }
        self.move_to(LockPhase::Artifact)?;
        let row=sqlx::query("UPDATE email_actions SET consumed_at=$4 WHERE token_hash=$1 AND user_id=$2 AND purpose=$3 AND consumed_at IS NULL AND expires_at>$4 RETURNING id")
            .bind(digest.as_bytes()).bind(self.user.id).bind(purpose.as_str()).bind(self.clock.now()).fetch_optional(&mut *self.transaction).await?;
        let result = row
            .map(|r| r.try_get("id").map_err(Into::into))
            .unwrap_or(Err(RepositoryError::Conflict));
        if result.is_err() {
            self.failed = true;
        } else if matches!(purpose, ActionPurpose::VerifyEmail) {
            self.consumed_verify_action = true;
        }
        result
    }

    pub async fn mark_email_verified(&mut self) -> Result<(), RepositoryError> {
        if !self.consumed_verify_action || self.verification_applied {
            self.failed = true;
            return Err(RepositoryError::InvalidState);
        }
        let result = sqlx::query("UPDATE users SET verified=true,updated_at=$2 WHERE id=$1")
            .bind(self.user.id)
            .bind(self.clock.now())
            .execute(&mut *self.transaction)
            .await?;
        if result.rows_affected() != 1 {
            self.failed = true;
            return Err(RepositoryError::Conflict);
        }
        self.user.verified = true;
        self.verification_applied = true;
        Ok(())
    }

    /// Registration uses this inside its user/action/outbox transaction, not as an endpoint.
    pub async fn commit(self) -> Result<(), RepositoryError> {
        if self.failed
            || (self.login_consumed && !self.login_session_created)
            || (self.token_issue_permitted
                && (!self.token_inserted || !self.inserted_access || !self.inserted_refresh))
            || (self.consumed_verify_action && !self.verification_applied)
        {
            self.transaction.rollback().await?;
            return Err(RepositoryError::InvalidState);
        }
        self.transaction.commit().await.map_err(Into::into)
    }
    pub async fn rollback(self) -> Result<(), RepositoryError> {
        self.transaction.rollback().await.map_err(Into::into)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum TokenKind {
    Access,
    Refresh,
}
impl TokenKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Access => "access",
            Self::Refresh => "refresh",
        }
    }
}

/// Data returned only after atomically consuming the authorization code.
/// The later OAuth service verifies the supplied PKCE verifier before invoking consumption.
pub struct AuthorizationCodeState {
    pub id: Uuid,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub nonce: String,
}
pub struct TokenInsert {
    pub id: Uuid,
    pub token_hash: Digest,
    pub kind: TokenKind,
    pub family_id: Uuid,
    pub family_expires_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
}

impl SecurityTransaction<'_> {
    pub async fn lock_authorization_code(
        &mut self,
        digest: Digest,
        redirect_uri: &str,
        challenge: &str,
    ) -> Result<AuthorizationCodeState, RepositoryError> {
        if self.locked_code_id.is_some() || self.locked_family_id.is_some() {
            self.failed = true;
            return Err(RepositoryError::InvalidState);
        }
        let grant_id = self.grant_id.ok_or(RepositoryError::InvalidState)?;
        self.move_to(LockPhase::Artifact)?;
        let row=sqlx::query("SELECT id,redirect_uri,code_challenge,nonce FROM authorization_codes WHERE code_hash=$1 AND grant_id=$2 AND redirect_uri=$3 AND code_challenge=$4 AND code_challenge_method='S256' AND consumed_at IS NULL AND expires_at>$5 FOR UPDATE")
            .bind(digest.as_bytes()).bind(grant_id).bind(redirect_uri).bind(challenge).bind(self.clock.now()).fetch_optional(&mut *self.transaction).await?;
        let Some(r) = row else {
            self.failed = true;
            return Err(RepositoryError::Conflict);
        };
        self.locked_code_id = Some(r.try_get("id")?);
        Ok(AuthorizationCodeState {
            id: r.try_get("id")?,
            redirect_uri: r.try_get("redirect_uri")?,
            code_challenge: r.try_get("code_challenge")?,
            nonce: r.try_get("nonce")?,
        })
    }
    pub async fn consume_authorization_code(&mut self, id: Uuid) -> Result<(), RepositoryError> {
        if self.locked_code_id != Some(id) {
            self.failed = true;
            return Err(RepositoryError::InvalidState);
        }
        let grant_id = self.grant_id.ok_or(RepositoryError::InvalidState)?;
        self.move_to(LockPhase::Artifact)?;
        let result=sqlx::query("UPDATE authorization_codes SET consumed_at=$3 WHERE id=$1 AND grant_id=$2 AND consumed_at IS NULL AND expires_at>$3")
            .bind(id).bind(grant_id).bind(self.clock.now()).execute(&mut *self.transaction).await?;
        if result.rows_affected() != 1 {
            self.failed = true;
            return Err(RepositoryError::Conflict);
        }
        self.token_issue_permitted = true;
        Ok(())
    }
    /// Lock the entire family in stable token-ID order after user/session/grant.
    pub async fn lock_refresh_family(&mut self, family_id: Uuid) -> Result<(), RepositoryError> {
        if self.locked_family_id.is_some() || self.locked_code_id.is_some() {
            self.failed = true;
            return Err(RepositoryError::InvalidState);
        }
        let grant_id = self.grant_id.ok_or(RepositoryError::InvalidState)?;
        self.move_to(LockPhase::Artifact)?;
        let rows = sqlx::query(
            "SELECT id,family_expires_at FROM oauth_tokens WHERE family_id=$1 AND grant_id=$2 ORDER BY id FOR UPDATE",
        )
        .bind(family_id)
        .bind(grant_id)
        .fetch_all(&mut *self.transaction)
        .await?;
        if rows.is_empty() {
            return Err(RepositoryError::NotFound);
        }
        let absolute: OffsetDateTime = rows[0].try_get("family_expires_at")?;
        for row in &rows {
            let expiration: OffsetDateTime = row.try_get("family_expires_at")?;
            if expiration != absolute {
                self.failed = true;
                return Err(RepositoryError::InvalidState);
            }
        }
        self.locked_family_id = Some(family_id);
        self.locked_family_expires_at = Some(absolute);
        Ok(())
    }
    pub async fn consume_refresh_token(
        &mut self,
        digest: Digest,
        family_id: Uuid,
    ) -> Result<(), RepositoryError> {
        if self.locked_family_id != Some(family_id) {
            self.failed = true;
            return Err(RepositoryError::InvalidState);
        }
        let grant_id = self.grant_id.ok_or(RepositoryError::InvalidState)?;
        self.move_to(LockPhase::Artifact)?;
        let now = self.clock.now();
        let row=sqlx::query("SELECT consumed_at,revoked_at,expires_at,family_expires_at FROM oauth_tokens WHERE token_hash=$1 AND family_id=$2 AND grant_id=$3 AND kind='refresh' FOR UPDATE")
            .bind(digest.as_bytes()).bind(family_id).bind(grant_id).fetch_optional(&mut *self.transaction).await?;
        let Some(row) = row else {
            return Err(RepositoryError::Conflict);
        };
        let consumed: Option<OffsetDateTime> = row.try_get("consumed_at")?;
        let revoked: Option<OffsetDateTime> = row.try_get("revoked_at")?;
        let expires: OffsetDateTime = row.try_get("expires_at")?;
        let absolute: OffsetDateTime = row.try_get("family_expires_at")?;
        if revoked.is_some() || absolute <= now {
            return Err(RepositoryError::Conflict);
        }
        if consumed.is_some() {
            return Err(RepositoryError::Replayed);
        }
        if expires <= now {
            return Err(RepositoryError::Conflict);
        }
        let result = sqlx::query(
            "UPDATE oauth_tokens SET consumed_at=$2 WHERE token_hash=$1 AND consumed_at IS NULL",
        )
        .bind(digest.as_bytes())
        .bind(now)
        .execute(&mut *self.transaction)
        .await?;
        if result.rows_affected() != 1 {
            return Err(RepositoryError::Conflict);
        }
        self.token_issue_permitted = true;
        Ok(())
    }
    pub async fn insert_token(&mut self, input: &TokenInsert) -> Result<(), RepositoryError> {
        let grant_id = self.grant_id.ok_or(RepositoryError::InvalidState)?;
        let now = self.clock.now();
        if self.failed
            || !self.token_issue_permitted
            || (matches!(input.kind, TokenKind::Access) && self.inserted_access)
            || (matches!(input.kind, TokenKind::Refresh) && self.inserted_refresh)
            || self
                .issued_family_id
                .is_some_and(|id| id != input.family_id)
            || input.expires_at <= now
            || input.expires_at > input.family_expires_at
            || input.family_expires_at <= now
            || self
                .locked_family_id
                .is_some_and(|id| id != input.family_id)
            || self
                .locked_family_expires_at
                .is_some_and(|expires| expires != input.family_expires_at)
        {
            self.failed = true;
            return Err(RepositoryError::InvalidState);
        }
        if self.locked_family_id.is_none() && self.issued_family_id.is_none() {
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM oauth_tokens WHERE family_id=$1)")
                    .bind(input.family_id)
                    .fetch_one(&mut *self.transaction)
                    .await?;
            if exists {
                self.failed = true;
                return Err(RepositoryError::InvalidState);
            }
        }
        sqlx::query("INSERT INTO oauth_tokens(id,token_hash,kind,grant_id,family_id,family_expires_at,expires_at,created_at) SELECT $1,$2,$3,g.id,$4,$5,$6,$7 FROM oauth_grants g JOIN sessions s ON s.id=g.session_id WHERE g.id=$8 AND g.revoked_at IS NULL AND s.revoked_at IS NULL AND $6<=g.expires_at AND $5<=g.expires_at AND $5<=s.expires_at")
            .bind(input.id).bind(input.token_hash.as_bytes()).bind(input.kind.as_str()).bind(input.family_id).bind(input.family_expires_at).bind(input.expires_at).bind(now).bind(grant_id).execute(&mut *self.transaction).await.and_then(|r|if r.rows_affected()==1{Ok(r)}else{Err(sqlx::Error::RowNotFound)})?;
        self.token_inserted = true;
        self.issued_family_id = Some(input.family_id);
        match input.kind {
            TokenKind::Access => self.inserted_access = true,
            TokenKind::Refresh => self.inserted_refresh = true,
        }
        Ok(())
    }
    /// Replay handling must COMMIT this revocation even when the caller returns invalid_grant.
    pub async fn revoke_refresh_family(&mut self, family_id: Uuid) -> Result<u64, RepositoryError> {
        if self.locked_family_id != Some(family_id) {
            self.failed = true;
            return Err(RepositoryError::InvalidState);
        }
        let grant_id = self.grant_id.ok_or(RepositoryError::InvalidState)?;
        self.move_to(LockPhase::Artifact)?;
        let result=sqlx::query("UPDATE oauth_tokens SET revoked_at=COALESCE(revoked_at,$3) WHERE family_id=$1 AND grant_id=$2")
            .bind(family_id).bind(grant_id).bind(self.clock.now()).execute(&mut *self.transaction).await?;
        Ok(result.rows_affected())
    }
    /// Ordered session/grant/token revocation; locks every affected row in deterministic ID order.
    pub async fn revoke_all_sessions(&mut self) -> Result<(), RepositoryError> {
        self.move_to(LockPhase::Session)?;
        sqlx::query("SELECT id FROM sessions WHERE user_id=$1 ORDER BY id FOR UPDATE")
            .bind(self.user.id)
            .fetch_all(&mut *self.transaction)
            .await?;
        self.move_to(LockPhase::Grant)?;
        sqlx::query("SELECT id FROM oauth_grants WHERE user_id=$1 ORDER BY id FOR UPDATE")
            .bind(self.user.id)
            .fetch_all(&mut *self.transaction)
            .await?;
        self.move_to(LockPhase::Artifact)?;
        sqlx::query("SELECT t.id FROM oauth_tokens t JOIN oauth_grants g ON g.id=t.grant_id WHERE g.user_id=$1 ORDER BY t.id FOR UPDATE OF t").bind(self.user.id).fetch_all(&mut *self.transaction).await?;
        let now = self.clock.now();
        sqlx::query("UPDATE sessions SET revoked_at=COALESCE(revoked_at,$2) WHERE user_id=$1")
            .bind(self.user.id)
            .bind(now)
            .execute(&mut *self.transaction)
            .await?;
        sqlx::query("UPDATE oauth_grants SET revoked_at=COALESCE(revoked_at,$2) WHERE user_id=$1")
            .bind(self.user.id)
            .bind(now)
            .execute(&mut *self.transaction)
            .await?;
        sqlx::query("UPDATE oauth_tokens SET revoked_at=COALESCE(revoked_at,$2) WHERE grant_id IN (SELECT id FROM oauth_grants WHERE user_id=$1)").bind(self.user.id).bind(now).execute(&mut *self.transaction).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn digest_requires_exactly_256_bits_and_never_formats_material() {
        assert_eq!(
            Digest::from_slice(&[3; 31]),
            Err(RepositoryError::InvalidState)
        );
        assert_eq!(
            Digest::from_slice(&[3; 33]),
            Err(RepositoryError::InvalidState)
        );
        assert!(Digest::from_slice(&[3; 32]).is_ok());
        assert_eq!(
            format!("{:?}", Digest::from_bytes([3; 32])),
            "Digest([REDACTED])"
        );
    }
    #[test]
    fn repository_error_never_exposes_internal_diagnostics() {
        let error = RepositoryError::from(sqlx::Error::Protocol(
            "password-secret sql internal.host".to_string(),
        ));
        assert_eq!(error, RepositoryError::Unavailable);
        assert!(!error.to_string().contains("password-secret"));
    }
}
