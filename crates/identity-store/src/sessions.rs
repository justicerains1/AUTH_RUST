//! Post-password-check authority commits. Raw passwords, session tokens and CSRF never enter SQL.
use crate::{
    repository::{Digest, RepositoryError},
    security::{AuditEvent, AuditRecord, AuditResult, AuditTarget, insert_audit},
};
use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use identity_core::{
    clock::Clock,
    security::{constant_time_equal, keyed_account_digest},
};
use serde::{Deserialize, Serialize, Serializer};
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use std::{fmt, sync::Arc};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionError {
    InvalidCredentials,
    InvalidContext,
    NotAuthenticated,
    NotFound,
    InvalidCursor,
    InvalidInput,
    Unavailable,
}
impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidCredentials => "invalid credentials",
            Self::InvalidContext => "browser context no longer valid",
            Self::NotAuthenticated => "session required",
            Self::NotFound => "session not found",
            Self::InvalidCursor => "invalid session cursor",
            Self::InvalidInput => "invalid session input",
            Self::Unavailable => "session storage unavailable",
        })
    }
}
impl std::error::Error for SessionError {}
impl From<sqlx::Error> for SessionError {
    fn from(_: sqlx::Error) -> Self {
        Self::Unavailable
    }
}
impl From<RepositoryError> for SessionError {
    fn from(_: RepositoryError) -> Self {
        Self::Unavailable
    }
}

pub struct LoginCredential {
    pub id: Uuid,
    pub email: String,
    pub password_hash: Zeroizing<String>,
    pub verified: bool,
    pub status: String,
    pub credential_version: i64,
    pub totp_enabled: bool,
}
impl fmt::Debug for LoginCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoginCredential")
            .field("id", &self.id)
            .field("password_hash", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}
pub struct LoginCommitInput {
    pub user_id: Uuid,
    pub expected_credential_version: i64,
    pub preauth_hash: Option<Digest>,
    pub old_session_hash: Option<Digest>,
    pub session_id: Uuid,
    pub session_token_hash: Digest,
    pub session_csrf_hash: Digest,
    pub new_preauth_hash: Digest,
    pub new_preauth_csrf_hash: Digest,
    pub user_agent: String,
    pub upgraded_password_hash: Option<Zeroizing<String>>,
    pub request_id: Uuid,
    pub source_hash: Digest,
}
pub struct LogoutInput {
    pub current_hash: Option<Digest>,
    pub all: bool,
    pub new_preauth_hash: Digest,
    pub new_preauth_csrf_hash: Digest,
    pub request_id: Uuid,
    pub source_hash: Digest,
}
pub struct SessionRevocationInput {
    pub current_hash: Digest,
    pub target_id: Uuid,
    pub request_id: Uuid,
    pub source_hash: Digest,
}
pub enum LoginOutcome {
    Authenticated {
        user: UserView,
        session: Box<SessionView>,
    },
    MfaRequired {
        challenge_id: Uuid,
        expires_at: OffsetDateTime,
        methods: Vec<String>,
    },
}

fn timestamp<S: Serializer>(value: &OffsetDateTime, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&value.format(&Rfc3339).map_err(serde::ser::Error::custom)?)
}
fn optional_timestamp<S: Serializer>(
    value: &Option<OffsetDateTime>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(value) => {
            serializer.serialize_some(&value.format(&Rfc3339).map_err(serde::ser::Error::custom)?)
        }
        None => serializer.serialize_none(),
    }
}
#[derive(Serialize)]
pub struct UserView {
    pub id: Uuid,
    pub sub: Uuid,
    pub email: String,
    pub email_verified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub status: String,
    #[serde(serialize_with = "timestamp")]
    pub created_at: OffsetDateTime,
}
#[derive(Serialize)]
pub struct SessionView {
    pub id: Uuid,
    pub amr: Vec<String>,
    #[serde(serialize_with = "timestamp")]
    pub auth_time: OffsetDateTime,
    #[serde(serialize_with = "optional_timestamp")]
    pub strong_at: Option<OffsetDateTime>,
    #[serde(serialize_with = "timestamp")]
    pub expires_at: OffsetDateTime,
    #[serde(serialize_with = "timestamp")]
    pub created_at: OffsetDateTime,
    pub user_agent: String,
    pub current: bool,
}
#[derive(Serialize)]
pub struct SecurityView {
    pub totp_enabled: bool,
    pub passkey_count: i64,
    pub recovery_codes_remaining: i64,
    pub is_admin: bool,
    pub admin_binding_only: bool,
}
#[derive(Serialize)]
pub struct MeView {
    pub user: UserView,
    pub session: SessionView,
    pub security: SecurityView,
}
#[derive(Serialize)]
pub struct SessionPageView {
    pub items: Vec<SessionView>,
    pub next_cursor: Option<String>,
}

#[derive(Clone)]
pub struct SessionService {
    pool: PgPool,
    clock: Arc<dyn Clock>,
}
impl SessionService {
    pub fn new(pool: PgPool, clock: Arc<dyn Clock>) -> Self {
        Self { pool, clock }
    }
    pub async fn credential_by_email(
        &self,
        email: &str,
    ) -> Result<Option<LoginCredential>, SessionError> {
        let row=sqlx::query("SELECT u.id,u.email,u.password_hash,u.verified,u.status,u.credential_version,EXISTS(SELECT 1 FROM totp_factors f WHERE f.user_id=u.id AND f.confirmed) AS totp_enabled FROM users u WHERE u.email=$1").bind(email).fetch_optional(&self.pool).await?;
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
    /// Caller has verified the password outside the transaction. Recheck version/status under user lock.
    pub async fn complete_password_login(
        &self,
        input: &LoginCommitInput,
    ) -> Result<LoginOutcome, SessionError> {
        if input.user_agent.chars().count() > 256 || input.user_agent.chars().any(char::is_control)
        {
            return Err(SessionError::InvalidInput);
        }
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let row=sqlx::query("SELECT id,email,display_name,verified,status,credential_version,created_at FROM users WHERE id=$1 FOR UPDATE").bind(input.user_id).fetch_optional(&mut *tx).await?.ok_or(SessionError::InvalidCredentials)?;
        let verified: bool = row.try_get("verified")?;
        let status: String = row.try_get("status")?;
        let version: i64 = row.try_get("credential_version")?;
        if !verified || status != "active" || version != input.expected_credential_version {
            return Err(SessionError::InvalidCredentials);
        }
        let user = user_view(&row)?;
        let mut old_ids = Vec::new();
        let mut valid_old_session = false;
        if let Some(old) = input.old_session_hash {
            let session = sqlx::query(
                "SELECT id,user_id,revoked_at,expires_at,credential_version FROM sessions WHERE token_hash=$1",
            )
            .bind(old.as_bytes())
            .fetch_optional(&mut *tx)
            .await?;
            if let Some(session) = session {
                let owner: Uuid = session.try_get("user_id")?;
                if owner != input.user_id {
                    return Err(SessionError::InvalidContext);
                }
                let revoked: Option<OffsetDateTime> = session.try_get("revoked_at")?;
                let expires: OffsetDateTime = session.try_get("expires_at")?;
                let old_version: i64 = session.try_get("credential_version")?;
                valid_old_session = revoked.is_none() && expires > now && old_version == version;
                old_ids.push(session.try_get("id")?);
            }
        }
        // Lock all affected pre-existing sessions, then grants/tokens in deterministic order.
        lock_revocation(&mut tx, input.user_id, &old_ids).await?;
        let valid_preauth = if let Some(preauth_hash) = input.preauth_hash {
            sqlx::query("SELECT id FROM preauthentication_contexts WHERE token_hash=$1 AND revoked_at IS NULL AND expires_at>$2 FOR UPDATE").bind(preauth_hash.as_bytes()).bind(now).fetch_optional(&mut *tx).await?.is_some()
        } else {
            false
        };
        if !valid_preauth && !valid_old_session {
            return Err(SessionError::InvalidContext);
        }
        let totp: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM totp_factors WHERE user_id=$1 AND confirmed)",
        )
        .bind(input.user_id)
        .fetch_one(&mut *tx)
        .await?;
        if let Some(hash) = &input.upgraded_password_hash {
            sqlx::query("UPDATE users SET password_hash=$2,updated_at=$3 WHERE id=$1")
                .bind(input.user_id)
                .bind(hash.as_str())
                .bind(now)
                .execute(&mut *tx)
                .await?;
        }
        // Preserve valid authorization contexts before revoking the old main session.
        let transaction_ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM authorization_transactions WHERE consumed_at IS NULL AND expires_at>$3 AND (($1::bytea IS NOT NULL AND preauth_hash=$1 AND session_id IS NULL) OR session_id=ANY($2::uuid[])) ORDER BY id FOR UPDATE")
            .bind(if valid_preauth { input.preauth_hash.as_ref().map(Digest::as_bytes) } else { None })
            .bind(if valid_old_session { old_ids.as_slice() } else { &[] })
            .bind(now).fetch_all(&mut *tx).await?;
        // TOTP must not leave an ordinary browser session valid before the second factor.
        revoke_locked_excluding_transactions(
            &mut tx,
            input.user_id,
            &old_ids,
            &transaction_ids,
            now,
        )
        .await?;
        if let Some(preauth_hash) = input.preauth_hash {
            sqlx::query("UPDATE authentication_challenges SET consumed_at=COALESCE(consumed_at,$2) WHERE preauth_hash=$1").bind(preauth_hash.as_bytes()).bind(now).execute(&mut *tx).await?;
            sqlx::query("UPDATE preauthentication_contexts SET revoked_at=COALESCE(revoked_at,$2) WHERE token_hash=$1").bind(preauth_hash.as_bytes()).bind(now).execute(&mut *tx).await?;
        }
        insert_preauth(
            &mut tx,
            input.new_preauth_hash,
            input.new_preauth_csrf_hash,
            now,
        )
        .await?;
        if totp {
            let challenge_id = Uuid::new_v4();
            let expires_at = now + Duration::minutes(5);
            sqlx::query("UPDATE authorization_transactions SET preauth_hash=$2,session_id=NULL WHERE id=ANY($1::uuid[])").bind(&transaction_ids).bind(input.new_preauth_hash.as_bytes()).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO authentication_challenges(id,user_id,preauth_hash,purpose,state_data,created_at,expires_at) VALUES($1,$2,$3,'login',$4,$5,$6)").bind(challenge_id).bind(input.user_id).bind(input.new_preauth_hash.as_bytes()).bind(serde_json::json!({"credential_version":version})).bind(now).bind(expires_at).execute(&mut *tx).await?;
            for id in &old_ids {
                audit(
                    &mut tx,
                    input.user_id,
                    AuditEvent::SessionRevoked,
                    AuditTarget::Session,
                    *id,
                    AuditContext {
                        request_id: input.request_id,
                        source: input.source_hash,
                        now,
                    },
                )
                .await?;
            }
            tx.commit().await?;
            return Ok(LoginOutcome::MfaRequired {
                challenge_id,
                expires_at,
                methods: vec!["totp".into(), "recovery_code".into()],
            });
        }
        let expires_at = now + Duration::hours(12);
        sqlx::query("INSERT INTO sessions(id,token_hash,user_id,amr,auth_time,strong_at,csrf_hash,credential_version,user_agent,created_at,expires_at) VALUES($1,$2,$3,ARRAY['pwd']::text[],$4,NULL,$5,$6,$7,$4,$8)").bind(input.session_id).bind(input.session_token_hash.as_bytes()).bind(input.user_id).bind(now).bind(input.session_csrf_hash.as_bytes()).bind(version).bind(&input.user_agent).bind(expires_at).execute(&mut *tx).await?;
        sqlx::query("UPDATE authorization_transactions SET preauth_hash=NULL,session_id=$2 WHERE id=ANY($1::uuid[])").bind(&transaction_ids).bind(input.session_id).execute(&mut *tx).await?;
        audit(
            &mut tx,
            input.user_id,
            AuditEvent::LoginSucceeded,
            AuditTarget::Session,
            input.session_id,
            AuditContext {
                request_id: input.request_id,
                source: input.source_hash,
                now,
            },
        )
        .await?;
        for id in &old_ids {
            audit(
                &mut tx,
                input.user_id,
                AuditEvent::SessionRevoked,
                AuditTarget::Session,
                *id,
                AuditContext {
                    request_id: input.request_id,
                    source: input.source_hash,
                    now,
                },
            )
            .await?;
        }
        tx.commit().await?;
        Ok(LoginOutcome::Authenticated {
            user,
            session: Box::new(SessionView {
                id: input.session_id,
                amr: vec!["pwd".into()],
                auth_time: now,
                strong_at: None,
                expires_at,
                created_at: now,
                user_agent: input.user_agent.clone(),
                current: true,
            }),
        })
    }

    pub async fn me(&self, hash: Digest) -> Result<Option<MeView>, SessionError> {
        let row=sqlx::query("SELECT u.id,u.email,u.display_name,u.verified,u.status,u.created_at,s.id AS session_id,s.amr,s.auth_time,s.strong_at,s.expires_at,s.created_at AS session_created_at,s.user_agent,EXISTS(SELECT 1 FROM totp_factors f WHERE f.user_id=u.id AND f.confirmed) AS totp_enabled,(SELECT count(*) FROM webauthn_credentials k WHERE k.user_id=u.id) AS passkey_count,(SELECT count(*) FROM recovery_codes r WHERE r.user_id=u.id AND r.consumed_at IS NULL) AS recovery_codes_remaining,EXISTS(SELECT 1 FROM admin_memberships a WHERE a.user_id=u.id AND a.enabled) AS is_admin FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=$1 AND s.revoked_at IS NULL AND s.expires_at>$2 AND s.credential_version=u.credential_version AND u.verified AND u.status='active'").bind(hash.as_bytes()).bind(self.clock.now()).fetch_optional(&self.pool).await?;
        row.map(|row| {
            let totp: bool = row.try_get("totp_enabled")?;
            let passkeys: i64 = row.try_get("passkey_count")?;
            let is_admin: bool = row.try_get("is_admin")?;
            Ok(MeView {
                user: user_view(&row)?,
                session: session_view(&row, row.try_get("session_id")?, true)?,
                security: SecurityView {
                    totp_enabled: totp,
                    passkey_count: passkeys,
                    recovery_codes_remaining: row.try_get("recovery_codes_remaining")?,
                    is_admin,
                    admin_binding_only: is_admin && !totp && passkeys == 0,
                },
            })
        })
        .transpose()
    }
    pub async fn session_page(
        &self,
        hash: Digest,
        limit: u32,
        cursor: Option<&str>,
        key: &[u8; 32],
    ) -> Result<SessionPageView, SessionError> {
        if !(1..=100).contains(&limit) {
            return Err(SessionError::InvalidInput);
        }
        let me = self.me(hash).await?.ok_or(SessionError::NotAuthenticated)?;
        let position = cursor
            .map(|cursor| decode_cursor(cursor, key, me.user.id, self.clock.now()))
            .transpose()?;
        let created = position.as_ref().map(|p| p.created_at);
        let id = position.as_ref().map(|p| p.id);
        let rows=sqlx::query("SELECT s.id AS session_id,s.amr,s.auth_time,s.strong_at,s.expires_at,s.created_at AS session_created_at,s.user_agent FROM sessions s JOIN users u ON u.id=s.user_id JOIN sessions current_session ON current_session.user_id=u.id AND current_session.token_hash=$6 AND current_session.revoked_at IS NULL AND current_session.expires_at>$2 AND current_session.credential_version=u.credential_version WHERE s.user_id=$1 AND s.revoked_at IS NULL AND s.expires_at>$2 AND s.credential_version=u.credential_version AND u.verified AND u.status='active' AND ($3::timestamptz IS NULL OR (s.created_at,s.id)<($3,$4::uuid)) ORDER BY s.created_at DESC,s.id DESC LIMIT $5").bind(me.user.id).bind(self.clock.now()).bind(created).bind(id).bind(i64::from(limit)+1).bind(hash.as_bytes()).fetch_all(&self.pool).await?;
        let has_more = rows.len() > limit as usize;
        let mut items = Vec::new();
        for row in rows.iter().take(limit as usize) {
            let id = row.try_get("session_id")?;
            items.push(session_view(row, id, id == me.session.id)?);
        }
        let next_cursor = if has_more {
            items
                .last()
                .map(|item| {
                    encode_cursor(me.user.id, item.created_at, item.id, self.clock.now(), key)
                })
                .transpose()?
        } else {
            None
        };
        Ok(SessionPageView { items, next_cursor })
    }
    pub async fn revoke_session(
        &self,
        input: &SessionRevocationInput,
    ) -> Result<bool, SessionError> {
        let now = self.clock.now();
        let (mut tx, user, current) = self.lock_current(input.current_hash, now).await?;
        let ids = if current == input.target_id {
            vec![current]
        } else {
            let mut ids = vec![current, input.target_id];
            ids.sort();
            ids
        };
        let target: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions WHERE id=$1 AND user_id=$2)")
                .bind(input.target_id)
                .bind(user)
                .fetch_one(&mut *tx)
                .await?;
        if !target {
            return Err(SessionError::NotFound);
        }
        lock_revocation(&mut tx, user, &ids).await?;
        revoke_locked(&mut tx, user, &[input.target_id], now).await?;
        audit(
            &mut tx,
            user,
            AuditEvent::SessionRevoked,
            AuditTarget::Session,
            input.target_id,
            AuditContext {
                request_id: input.request_id,
                source: input.source_hash,
                now,
            },
        )
        .await?;
        tx.commit().await?;
        Ok(current == input.target_id)
    }
    pub async fn logout(&self, input: &LogoutInput) -> Result<bool, SessionError> {
        let Some(hash) = input.current_hash else {
            return Ok(false);
        };
        let now = self.clock.now();
        let (mut tx, user, current) = match self.lock_current(hash, now).await {
            Ok(value) => value,
            Err(SessionError::NotAuthenticated) => return Ok(false),
            Err(error) => return Err(error),
        };
        let ids: Vec<Uuid> = if input.all {
            sqlx::query_scalar("SELECT id FROM sessions WHERE user_id=$1 ORDER BY id")
                .bind(user)
                .fetch_all(&mut *tx)
                .await?
        } else {
            vec![current]
        };
        lock_revocation(&mut tx, user, &ids).await?;
        revoke_locked(&mut tx, user, &ids, now).await?;
        insert_preauth(
            &mut tx,
            input.new_preauth_hash,
            input.new_preauth_csrf_hash,
            now,
        )
        .await?;
        audit(
            &mut tx,
            user,
            if input.all {
                AuditEvent::SessionsRevoked
            } else {
                AuditEvent::SessionRevoked
            },
            AuditTarget::Session,
            current,
            AuditContext {
                request_id: input.request_id,
                source: input.source_hash,
                now,
            },
        )
        .await?;
        tx.commit().await?;
        Ok(true)
    }
    async fn lock_current(
        &self,
        hash: Digest,
        now: OffsetDateTime,
    ) -> Result<(Transaction<'_, Postgres>, Uuid, Uuid), SessionError> {
        let user: Option<Uuid> =
            sqlx::query_scalar("SELECT user_id FROM sessions WHERE token_hash=$1")
                .bind(hash.as_bytes())
                .fetch_optional(&self.pool)
                .await?;
        let user = user.ok_or(SessionError::NotAuthenticated)?;
        let mut tx = self.pool.begin().await?;
        let valid:Option<i64>=sqlx::query_scalar("SELECT credential_version FROM users WHERE id=$1 AND verified AND status='active' FOR UPDATE").bind(user).fetch_optional(&mut *tx).await?;
        let version = valid.ok_or(SessionError::NotAuthenticated)?;
        let current:Option<Uuid>=sqlx::query_scalar("SELECT id FROM sessions WHERE token_hash=$1 AND user_id=$2 AND credential_version=$3 AND revoked_at IS NULL AND expires_at>$4").bind(hash.as_bytes()).bind(user).bind(version).bind(now).fetch_optional(&mut *tx).await?;
        Ok((tx, user, current.ok_or(SessionError::NotAuthenticated)?))
    }
}

pub(crate) fn user_view(row: &PgRow) -> Result<UserView, SessionError> {
    let id = row.try_get("id")?;
    Ok(UserView {
        id,
        sub: id,
        email: row.try_get("email")?,
        email_verified: row.try_get("verified")?,
        display_name: row.try_get("display_name")?,
        status: row.try_get("status")?,
        created_at: row.try_get("created_at")?,
    })
}
fn session_view(row: &PgRow, id: Uuid, current: bool) -> Result<SessionView, SessionError> {
    Ok(SessionView {
        id,
        amr: row.try_get("amr")?,
        auth_time: row.try_get("auth_time")?,
        strong_at: row.try_get("strong_at")?,
        expires_at: row.try_get("expires_at")?,
        created_at: row.try_get("session_created_at")?,
        user_agent: row.try_get("user_agent")?,
        current,
    })
}
pub(crate) async fn lock_revocation(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
    ids: &[Uuid],
) -> Result<(), SessionError> {
    sqlx::query(
        "SELECT id FROM sessions WHERE user_id=$1 AND id=ANY($2::uuid[]) ORDER BY id FOR UPDATE",
    )
    .bind(user)
    .bind(ids)
    .fetch_all(&mut **tx)
    .await?;
    sqlx::query("SELECT id FROM oauth_grants WHERE user_id=$1 AND session_id=ANY($2::uuid[]) ORDER BY id FOR UPDATE").bind(user).bind(ids).fetch_all(&mut **tx).await?;
    sqlx::query("SELECT t.id FROM oauth_tokens t JOIN oauth_grants g ON g.id=t.grant_id WHERE g.user_id=$1 AND g.session_id=ANY($2::uuid[]) ORDER BY t.id FOR UPDATE OF t").bind(user).bind(ids).fetch_all(&mut **tx).await?;
    sqlx::query("SELECT c.id FROM authorization_codes c JOIN oauth_grants g ON g.id=c.grant_id WHERE g.user_id=$1 AND g.session_id=ANY($2::uuid[]) ORDER BY c.id FOR UPDATE OF c").bind(user).bind(ids).fetch_all(&mut **tx).await?;
    Ok(())
}
pub(crate) async fn revoke_locked(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
    ids: &[Uuid],
    now: OffsetDateTime,
) -> Result<(), SessionError> {
    revoke_locked_excluding_transactions(tx, user, ids, &[], now).await
}
async fn revoke_locked_excluding_transactions(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
    ids: &[Uuid],
    preserved_transactions: &[Uuid],
    now: OffsetDateTime,
) -> Result<(), SessionError> {
    sqlx::query("UPDATE sessions SET revoked_at=COALESCE(revoked_at,$3) WHERE user_id=$1 AND id=ANY($2::uuid[])").bind(user).bind(ids).bind(now).execute(&mut **tx).await?;
    sqlx::query("UPDATE oauth_grants SET revoked_at=COALESCE(revoked_at,$3) WHERE user_id=$1 AND session_id=ANY($2::uuid[])").bind(user).bind(ids).bind(now).execute(&mut **tx).await?;
    sqlx::query("UPDATE oauth_tokens SET revoked_at=COALESCE(revoked_at,$3) WHERE grant_id IN(SELECT id FROM oauth_grants WHERE user_id=$1 AND session_id=ANY($2::uuid[]))").bind(user).bind(ids).bind(now).execute(&mut **tx).await?;
    sqlx::query("UPDATE authorization_transactions SET consumed_at=COALESCE(consumed_at,$3) WHERE session_id=ANY($2::uuid[]) AND session_id IN(SELECT id FROM sessions WHERE user_id=$1) AND NOT(id=ANY($4::uuid[]))").bind(user).bind(ids).bind(now).bind(preserved_transactions).execute(&mut **tx).await?;
    sqlx::query("UPDATE authentication_challenges SET consumed_at=COALESCE(consumed_at,$3) WHERE user_id=$1 AND session_id=ANY($2::uuid[])").bind(user).bind(ids).bind(now).execute(&mut **tx).await?;
    Ok(())
}
pub(crate) async fn insert_preauth(
    tx: &mut Transaction<'_, Postgres>,
    token: Digest,
    csrf: Digest,
    now: OffsetDateTime,
) -> Result<(), SessionError> {
    sqlx::query("INSERT INTO preauthentication_contexts(id,token_hash,csrf_hash,created_at,expires_at) VALUES($1,$2,$3,$4,$5)").bind(Uuid::new_v4()).bind(token.as_bytes()).bind(csrf.as_bytes()).bind(now).bind(now+Duration::minutes(10)).execute(&mut **tx).await?;
    Ok(())
}
struct AuditContext {
    request_id: Uuid,
    source: Digest,
    now: OffsetDateTime,
}
async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
    event: AuditEvent,
    target: AuditTarget,
    id: Uuid,
    context: AuditContext,
) -> Result<(), SessionError> {
    insert_audit(
        tx,
        &AuditRecord {
            id: Uuid::new_v4(),
            event,
            actor_id: Some(user),
            target,
            target_id: Some(id),
            result: AuditResult::Success,
            request_id: context.request_id,
            source_hash: context.source,
            occurred_at: context.now,
        },
    )
    .await?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CursorPayload {
    version: u8,
    user: Uuid,
    route: String,
    created_at: OffsetDateTime,
    id: Uuid,
    expires_at: OffsetDateTime,
}
fn encode_cursor(
    user: Uuid,
    created_at: OffsetDateTime,
    id: Uuid,
    now: OffsetDateTime,
    key: &[u8; 32],
) -> Result<String, SessionError> {
    let payload = CursorPayload {
        version: 1,
        user,
        route: "me/sessions:created_desc,id_desc".into(),
        created_at,
        id,
        expires_at: now + Duration::minutes(10),
    };
    let json = serde_json::to_string(&payload).map_err(|_| SessionError::InvalidCursor)?;
    let signature = keyed_account_digest(key, "identity-session-cursor-v1", &json);
    Ok(format!(
        "{}.{}",
        BASE64_URL_SAFE_NO_PAD.encode(json),
        BASE64_URL_SAFE_NO_PAD.encode(signature)
    ))
}
fn decode_cursor(
    cursor: &str,
    key: &[u8; 32],
    user: Uuid,
    now: OffsetDateTime,
) -> Result<CursorPayload, SessionError> {
    if cursor.len() > 2048 {
        return Err(SessionError::InvalidCursor);
    }
    let (payload, signature) = cursor.split_once('.').ok_or(SessionError::InvalidCursor)?;
    let raw = BASE64_URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| SessionError::InvalidCursor)?;
    let json = std::str::from_utf8(&raw).map_err(|_| SessionError::InvalidCursor)?;
    let actual = BASE64_URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| SessionError::InvalidCursor)?;
    if !constant_time_equal(
        &actual,
        &keyed_account_digest(key, "identity-session-cursor-v1", json),
    ) {
        return Err(SessionError::InvalidCursor);
    }
    let payload: CursorPayload =
        serde_json::from_str(json).map_err(|_| SessionError::InvalidCursor)?;
    if payload.version != 1
        || payload.user != user
        || payload.route != "me/sessions:created_desc,id_desc"
        || payload.expires_at <= now
    {
        return Err(SessionError::InvalidCursor);
    }
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_is_user_sort_expiry_and_signature_bound() -> Result<(), SessionError> {
        let now = OffsetDateTime::UNIX_EPOCH + Duration::days(21000);
        let user = Uuid::new_v4();
        let cursor = encode_cursor(user, now, Uuid::new_v4(), now, &[7; 32])?;
        assert!(decode_cursor(&cursor, &[7; 32], user, now).is_ok());
        assert!(decode_cursor(&cursor, &[8; 32], user, now).is_err());
        assert!(decode_cursor(&cursor, &[7; 32], Uuid::new_v4(), now).is_err());
        assert!(decode_cursor(&cursor, &[7; 32], user, now + Duration::minutes(10)).is_err());
        assert!(decode_cursor(&(cursor + "x"), &[7; 32], user, now).is_err());
        Ok(())
    }
}
