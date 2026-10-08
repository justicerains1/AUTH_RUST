//! Platform management transactions. Administrator protection lock always precedes user rows.
use crate::{
    repository::Digest,
    security::{AuditEvent, AuditRecord, AuditResult, AuditTarget, insert_audit},
    sessions::{UserView, lock_revocation, revoke_locked, user_view},
};
use identity_core::{
    clock::Clock,
    oauth::{Scope, validate_redirect_uri},
    security::{Token, token_digest},
};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use std::{collections::BTreeSet, fmt, sync::Arc};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;
use zeroize::Zeroizing;

const ADMIN_LOCK: i64 = 0x4944454e5441444d;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminError {
    Forbidden,
    StrongRequired,
    LastAdministrator,
    AlreadyInitialized,
    InvalidBootstrap,
    NotFound,
    InvalidInput,
    TargetIneligible,
    StateConflict,
    Unavailable,
}
impl fmt::Display for AdminError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Forbidden => "administrator permission required",
            Self::StrongRequired => "recent strong authentication required",
            Self::LastAdministrator => "last available administrator is protected",
            Self::AlreadyInitialized => "administrator already initialized",
            Self::InvalidBootstrap => "administrator bootstrap rejected",
            Self::NotFound => "resource not found",
            Self::InvalidInput => "invalid administrator input",
            Self::TargetIneligible => "administrator target is ineligible",
            Self::StateConflict => "administrator state conflict",
            Self::Unavailable => "administrator storage unavailable",
        })
    }
}
impl std::error::Error for AdminError {}
impl From<sqlx::Error> for AdminError {
    fn from(_: sqlx::Error) -> Self {
        Self::Unavailable
    }
}
#[derive(Clone, Copy)]
pub struct AdminContext {
    pub session_hash: Digest,
    pub request_id: Uuid,
    pub source_hash: Digest,
}
pub struct BootstrapInput {
    pub email: String,
    pub password_hash: Option<Zeroizing<String>>,
    pub expected_existing_version: Option<i64>,
    pub request_id: Uuid,
    pub source_hash: Digest,
}
pub struct VerifiedBootstrap {
    input: BootstrapInput,
}
pub struct BootstrapResult {
    pub user_id: Uuid,
    pub binding_only: bool,
}
pub struct AdminIdentity {
    pub user_id: Uuid,
    pub session_id: Uuid,
}
#[derive(Serialize)]
pub struct MemberView {
    pub user_id: Uuid,
    pub email: String,
    pub enabled: bool,
    pub created_at: String,
}
#[derive(Serialize)]
pub struct ClientView {
    pub id: Uuid,
    pub client_id: String,
    pub name: String,
    pub enabled: bool,
    pub allowed_scopes: Vec<String>,
    pub redirect_uris: Vec<String>,
    pub post_logout_redirect_uris: Vec<String>,
    pub created_at: String,
}
pub struct ClientCreateInput {
    pub name: String,
    pub allowed_scopes: Vec<Scope>,
    pub redirect_uris: Vec<String>,
    pub post_logout_redirect_uris: Vec<String>,
    pub production: bool,
}
pub struct ClientUpdateInput {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub allowed_scopes: Option<Vec<Scope>>,
    pub redirect_uris: Option<Vec<String>>,
    pub post_logout_redirect_uris: Option<Vec<String>>,
    pub production: bool,
}
pub struct ClientSecretOnce {
    pub client: ClientView,
    pub client_secret: Token,
}
#[derive(Serialize)]
pub struct AdminPage<T: Serialize> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}
#[derive(Default)]
pub struct UserFilter {
    pub email: Option<String>,
    pub status: Option<String>,
}
#[derive(Default)]
pub struct AuditWindow {
    pub from: Option<OffsetDateTime>,
    pub to: Option<OffsetDateTime>,
}
#[derive(Serialize)]
pub struct AuditView {
    pub id: Uuid,
    pub event: String,
    pub actor_user_id: Option<Uuid>,
    pub target_type: String,
    pub target_id: Option<Uuid>,
    pub result: String,
    pub request_id: Uuid,
    pub source: String,
    pub occurred_at: String,
}
#[derive(Clone)]
pub struct AdminStore {
    pool: PgPool,
    clock: Arc<dyn Clock>,
}
impl AdminStore {
    pub fn new(pool: PgPool, clock: Arc<dyn Clock>) -> Self {
        Self { pool, clock }
    }
    /// Bootstrap proof is produced only after a real local password check; it is not an HTTP DTO.
    pub async fn verify_bootstrap(
        &self,
        email: String,
        password: &str,
        hashing: &identity_core::security::PasswordService,
        request_id: Uuid,
        source_hash: Digest,
    ) -> Result<VerifiedBootstrap, AdminError> {
        let email = identity_core::security::normalize_email(&email)
            .map_err(|_| AdminError::InvalidBootstrap)?;
        let service = crate::sessions::SessionService::new(self.pool.clone(), self.clock.clone());
        let existing = service
            .credential_by_email(&email)
            .await
            .map_err(|_| AdminError::Unavailable)?;
        let (version, hash) = if let Some(existing) = existing {
            if !existing.verified
                || existing.status != "active"
                || !hashing
                    .verify(password, &existing.password_hash)
                    .await
                    .map_err(|_| AdminError::Unavailable)?
                    .valid
            {
                return Err(AdminError::InvalidBootstrap);
            }
            (Some(existing.credential_version), None)
        } else {
            let password = identity_core::security::Password::new(password)
                .map_err(|_| AdminError::InvalidBootstrap)?;
            (
                None,
                Some(
                    hashing
                        .hash(&password)
                        .await
                        .map_err(|_| AdminError::Unavailable)?,
                ),
            )
        };
        Ok(VerifiedBootstrap {
            input: BootstrapInput {
                email,
                password_hash: hash,
                expected_existing_version: version,
                request_id,
                source_hash,
            },
        })
    }
    pub async fn bootstrap_verified(
        &self,
        proof: &VerifiedBootstrap,
    ) -> Result<BootstrapResult, AdminError> {
        self.bootstrap(&proof.input).await
    }
    async fn bootstrap(&self, input: &BootstrapInput) -> Result<BootstrapResult, AdminError> {
        let mut tx = self.pool.begin().await?;
        guard(&mut tx).await?;
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM admin_memberships)")
            .fetch_one(&mut *tx)
            .await?;
        if exists {
            return Err(AdminError::AlreadyInitialized);
        }
        let now = self.clock.now();
        let user = sqlx::query(
            "SELECT id,verified,status,credential_version FROM users WHERE email=$1 FOR UPDATE",
        )
        .bind(&input.email)
        .fetch_optional(&mut *tx)
        .await?;
        let id = if let Some(user) = user {
            if !user.try_get::<bool, _>("verified")?
                || user.try_get::<String, _>("status")? != "active"
                || Some(user.try_get::<i64, _>("credential_version")?)
                    != input.expected_existing_version
            {
                return Err(AdminError::InvalidBootstrap);
            }
            user.try_get("id")?
        } else {
            let hash = input
                .password_hash
                .as_ref()
                .ok_or(AdminError::InvalidBootstrap)?;
            let id = Uuid::new_v4();
            sqlx::query("INSERT INTO users(id,email,password_hash,verified,created_at,updated_at) VALUES($1,$2,$3,true,$4,$4)").bind(id).bind(&input.email).bind(hash.as_str()).bind(now).execute(&mut *tx).await?;
            id
        };
        sqlx::query(
            "INSERT INTO admin_memberships(id,user_id,enabled,created_at) VALUES($1,$2,true,$3)",
        )
        .bind(Uuid::new_v4())
        .bind(id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        audit(
            &mut tx,
            id,
            AuditEvent::AdminInitialized,
            AuditTarget::AdminMember,
            id,
            AuditContext {
                request_id: input.request_id,
                source: input.source_hash,
                now,
            },
        )
        .await?;
        let factor = has_factor(&mut tx, id).await?;
        tx.commit().await?;
        Ok(BootstrapResult {
            user_id: id,
            binding_only: !factor,
        })
    }
    pub async fn authorize(&self, context: AdminContext) -> Result<AdminIdentity, AdminError> {
        let mut tx = self.pool.begin().await?;
        let identity = authorize(&mut tx, context.session_hash, self.clock.now()).await?;
        tx.commit().await?;
        Ok(identity)
    }
    pub async fn grant_member(
        &self,
        context: AdminContext,
        target: Uuid,
    ) -> Result<MemberView, AdminError> {
        let (mut tx, actor) = self.begin(context, &[target]).await?;
        let now = self.clock.now();
        let user = sqlx::query("SELECT email,verified,status FROM users WHERE id=$1")
            .bind(target)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(AdminError::TargetIneligible)?;
        if !user.try_get::<bool, _>("verified")?
            || user.try_get::<String, _>("status")? != "active"
            || !has_factor(&mut tx, target).await?
        {
            return Err(AdminError::TargetIneligible);
        }
        let duplicate: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM admin_memberships WHERE user_id=$1 AND enabled)",
        )
        .bind(target)
        .fetch_one(&mut *tx)
        .await?;
        if duplicate {
            return Err(AdminError::StateConflict);
        }
        sqlx::query("INSERT INTO admin_memberships(id,user_id,enabled,created_at) VALUES($1,$2,true,$3) ON CONFLICT(user_id) DO UPDATE SET enabled=true").bind(Uuid::new_v4()).bind(target).bind(now).execute(&mut *tx).await?;
        audit_ctx(
            &mut tx,
            actor.user_id,
            AuditEvent::AdminAdded,
            AuditTarget::AdminMember,
            target,
            context,
            now,
        )
        .await?;
        let row=sqlx::query("SELECT a.user_id,u.email,a.enabled,a.created_at FROM admin_memberships a JOIN users u ON u.id=a.user_id WHERE a.user_id=$1").bind(target).fetch_one(&mut *tx).await?;
        let result = member_view(&row)?;
        tx.commit().await?;
        Ok(result)
    }
    pub async fn remove_member(
        &self,
        context: AdminContext,
        target: Uuid,
    ) -> Result<(), AdminError> {
        let (mut tx, actor) = self.begin(context, &[target]).await?;
        match protect_last(&mut tx, target).await {
            Ok(()) => {}
            Err(AdminError::LastAdministrator) => {
                denied_last(&mut tx, actor.user_id, target, context, self.clock.now()).await?;
                tx.commit().await?;
                return Err(AdminError::LastAdministrator);
            }
            Err(error) => return Err(error),
        }
        let result = sqlx::query("DELETE FROM admin_memberships WHERE user_id=$1")
            .bind(target)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            return Err(AdminError::NotFound);
        }
        audit_ctx(
            &mut tx,
            actor.user_id,
            AuditEvent::AdminRemoved,
            AuditTarget::AdminMember,
            target,
            context,
            self.clock.now(),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn set_user_status(
        &self,
        context: AdminContext,
        target: Uuid,
        enabled: bool,
    ) -> Result<UserView, AdminError> {
        let (mut tx, actor) = self.begin(context, &[target]).await?;
        if !enabled {
            match protect_last(&mut tx, target).await {
                Ok(()) => {}
                Err(AdminError::LastAdministrator) => {
                    denied_last(&mut tx, actor.user_id, target, context, self.clock.now()).await?;
                    tx.commit().await?;
                    return Err(AdminError::LastAdministrator);
                }
                Err(error) => return Err(error),
            }
        }
        let now = self.clock.now();
        let ids: Vec<Uuid> =
            sqlx::query_scalar("SELECT id FROM sessions WHERE user_id=$1 ORDER BY id")
                .bind(target)
                .fetch_all(&mut *tx)
                .await?;
        lock_revocation(&mut tx, target, &ids)
            .await
            .map_err(|_| AdminError::Unavailable)?;
        let row=sqlx::query("UPDATE users SET status=$2,updated_at=$3 WHERE id=$1 RETURNING id,email,display_name,verified,status,created_at").bind(target).bind(if enabled{"active"}else{"disabled"}).bind(now).fetch_optional(&mut *tx).await?.ok_or(AdminError::NotFound)?;
        if !enabled {
            revoke_locked(&mut tx, target, &ids, now)
                .await
                .map_err(|_| AdminError::Unavailable)?;
        }
        audit_ctx(
            &mut tx,
            actor.user_id,
            if enabled {
                AuditEvent::UserEnabled
            } else {
                AuditEvent::UserStatusChanged
            },
            AuditTarget::User,
            target,
            context,
            now,
        )
        .await?;
        let result = user_view(&row).map_err(|_| AdminError::Unavailable)?;
        tx.commit().await?;
        Ok(result)
    }
    pub async fn revoke_user_sessions(
        &self,
        context: AdminContext,
        target: Uuid,
    ) -> Result<u64, AdminError> {
        let (mut tx, actor) = self.begin(context, &[target]).await?;
        let now = self.clock.now();
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id=$1)")
            .bind(target)
            .fetch_one(&mut *tx)
            .await?;
        if !exists {
            return Err(AdminError::NotFound);
        }
        let active_count:i64=sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id=$1 AND revoked_at IS NULL AND expires_at>$2").bind(target).bind(now).fetch_one(&mut *tx).await?;
        let ids: Vec<Uuid> =
            sqlx::query_scalar("SELECT id FROM sessions WHERE user_id=$1 ORDER BY id")
                .bind(target)
                .fetch_all(&mut *tx)
                .await?;
        lock_revocation(&mut tx, target, &ids)
            .await
            .map_err(|_| AdminError::Unavailable)?;
        revoke_locked(&mut tx, target, &ids, now)
            .await
            .map_err(|_| AdminError::Unavailable)?;
        audit_ctx(
            &mut tx,
            actor.user_id,
            AuditEvent::UserSessionsRevoked,
            AuditTarget::User,
            target,
            context,
            now,
        )
        .await?;
        tx.commit().await?;
        u64::try_from(active_count).map_err(|_| AdminError::Unavailable)
    }
    pub async fn create_client(
        &self,
        context: AdminContext,
        input: &ClientCreateInput,
    ) -> Result<ClientSecretOnce, AdminError> {
        validate_client(
            &input.name,
            &input.allowed_scopes,
            &input.redirect_uris,
            &input.post_logout_redirect_uris,
            input.production,
        )?;
        let (mut tx, actor) = self.begin(context, &[]).await?;
        let now = self.clock.now();
        let id = Uuid::new_v4();
        let client_id = format!("client-{}", id.simple());
        let secret = Token::generate().map_err(|_| AdminError::Unavailable)?;
        sqlx::query("INSERT INTO oauth_clients(id,client_id,name,secret_hash,allowed_scopes,created_at,updated_at) VALUES($1,$2,$3,$4,$5,$6,$6)").bind(id).bind(&client_id).bind(&input.name).bind(token_digest(secret.expose()).as_slice()).bind(scope_strings(&input.allowed_scopes)).bind(now).execute(&mut *tx).await?;
        replace_uris(
            &mut tx,
            id,
            &input.redirect_uris,
            &input.post_logout_redirect_uris,
        )
        .await?;
        audit_ctx(
            &mut tx,
            actor.user_id,
            AuditEvent::ClientCreated,
            AuditTarget::Client,
            id,
            context,
            now,
        )
        .await?;
        let client = client_view(&mut tx, id).await?;
        tx.commit().await?;
        Ok(ClientSecretOnce {
            client,
            client_secret: secret,
        })
    }
    pub async fn update_client(
        &self,
        context: AdminContext,
        id: Uuid,
        input: &ClientUpdateInput,
    ) -> Result<ClientView, AdminError> {
        let (mut tx, actor) = self.begin(context, &[]).await?;
        let previous = client_view(&mut tx, id).await?;
        let name = input.name.clone().unwrap_or(previous.name);
        let scopes = input
            .allowed_scopes
            .clone()
            .unwrap_or(parse_scopes(&previous.allowed_scopes)?);
        let redirects = input
            .redirect_uris
            .clone()
            .unwrap_or(previous.redirect_uris);
        let logout = input
            .post_logout_redirect_uris
            .clone()
            .unwrap_or(previous.post_logout_redirect_uris);
        validate_client(&name, &scopes, &redirects, &logout, input.production)?;
        let now = self.clock.now();
        sqlx::query("SELECT id FROM oauth_clients WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        sqlx::query("UPDATE oauth_clients SET name=$2,enabled=$3,allowed_scopes=$4,updated_at=$5 WHERE id=$1").bind(id).bind(name).bind(input.enabled.unwrap_or(previous.enabled)).bind(scope_strings(&scopes)).bind(now).execute(&mut *tx).await?;
        replace_uris(&mut tx, id, &redirects, &logout).await?;
        audit_ctx(
            &mut tx,
            actor.user_id,
            AuditEvent::ClientUpdated,
            AuditTarget::Client,
            id,
            context,
            now,
        )
        .await?;
        let result = client_view(&mut tx, id).await?;
        tx.commit().await?;
        Ok(result)
    }
    pub async fn rotate_client_secret(
        &self,
        context: AdminContext,
        id: Uuid,
    ) -> Result<ClientSecretOnce, AdminError> {
        let (mut tx, actor) = self.begin(context, &[]).await?;
        let secret = Token::generate().map_err(|_| AdminError::Unavailable)?;
        let now = self.clock.now();
        let result =
            sqlx::query("UPDATE oauth_clients SET secret_hash=$2,updated_at=$3 WHERE id=$1")
                .bind(id)
                .bind(token_digest(secret.expose()).as_slice())
                .bind(now)
                .execute(&mut *tx)
                .await?;
        if result.rows_affected() != 1 {
            return Err(AdminError::NotFound);
        }
        audit_ctx(
            &mut tx,
            actor.user_id,
            AuditEvent::ClientSecretRotated,
            AuditTarget::Client,
            id,
            context,
            now,
        )
        .await?;
        let client = client_view(&mut tx, id).await?;
        tx.commit().await?;
        Ok(ClientSecretOnce {
            client,
            client_secret: secret,
        })
    }
    pub async fn user(&self, context: AdminContext, id: Uuid) -> Result<UserView, AdminError> {
        self.authorize(context).await?;
        let row = sqlx::query(
            "SELECT id,email,display_name,verified,status,created_at FROM users WHERE id=$1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(AdminError::NotFound)?;
        user_view(&row).map_err(|_| AdminError::Unavailable)
    }
    pub async fn client(&self, context: AdminContext, id: Uuid) -> Result<ClientView, AdminError> {
        let mut tx = self.pool.begin().await?;
        authorize(&mut tx, context.session_hash, self.clock.now()).await?;
        client_view(&mut tx, id).await
    }
    pub async fn users(
        &self,
        context: AdminContext,
        limit: u32,
        cursor: Option<&str>,
        key: &[u8; 32],
    ) -> Result<AdminPage<UserView>, AdminError> {
        self.filtered_users(context, limit, cursor, key, &UserFilter::default())
            .await
    }
    pub async fn filtered_users(
        &self,
        context: AdminContext,
        limit: u32,
        cursor: Option<&str>,
        key: &[u8; 32],
        filter: &UserFilter,
    ) -> Result<AdminPage<UserView>, AdminError> {
        if filter
            .email
            .as_ref()
            .is_some_and(|email| identity_core::security::normalize_email(email).is_err())
            || filter
                .status
                .as_ref()
                .is_some_and(|status| !matches!(status.as_str(), "active" | "disabled"))
        {
            return Err(AdminError::InvalidInput);
        }
        let route = filter_route(
            "admin/users",
            &(filter.email.as_deref(), filter.status.as_deref()),
        )?;
        let mut tx = self.pool.begin().await?;
        let actor = authorize(&mut tx, context.session_hash, self.clock.now()).await?;
        let position = page_position(limit, cursor, key, actor.user_id, &route, self.clock.now())?;
        let rows=sqlx::query("SELECT id,email,display_name,verified,status,created_at FROM users WHERE ($1::timestamptz IS NULL OR (created_at,id)<($1,$2::uuid)) AND ($4::text IS NULL OR email=$4) AND ($5::text IS NULL OR status=$5) ORDER BY created_at DESC,id DESC LIMIT $3").bind(position.as_ref().map(|p|p.created_at)).bind(position.as_ref().map(|p|p.id)).bind(i64::from(limit)+1).bind(filter.email.as_deref()).bind(filter.status.as_deref()).fetch_all(&mut *tx).await?;
        let next = page_cursor(&rows, limit, key, actor.user_id, &route, self.clock.now())?;
        let items = rows
            .iter()
            .take(limit as usize)
            .map(|row| user_view(row).map_err(|_| AdminError::Unavailable))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(AdminPage {
            items,
            next_cursor: next,
        })
    }
    pub async fn members(
        &self,
        context: AdminContext,
        limit: u32,
        cursor: Option<&str>,
        key: &[u8; 32],
    ) -> Result<AdminPage<MemberView>, AdminError> {
        let mut tx = self.pool.begin().await?;
        let actor = authorize(&mut tx, context.session_hash, self.clock.now()).await?;
        let position = page_position(
            limit,
            cursor,
            key,
            actor.user_id,
            "admin/members",
            self.clock.now(),
        )?;
        let rows=sqlx::query("SELECT a.id,a.user_id,u.email,a.enabled,a.created_at FROM admin_memberships a JOIN users u ON u.id=a.user_id WHERE ($1::timestamptz IS NULL OR (a.created_at,a.id)<($1,$2::uuid)) ORDER BY a.created_at DESC,a.id DESC LIMIT $3").bind(position.as_ref().map(|p|p.created_at)).bind(position.as_ref().map(|p|p.id)).bind(i64::from(limit)+1).fetch_all(&mut *tx).await?;
        let next = page_cursor(
            &rows,
            limit,
            key,
            actor.user_id,
            "admin/members",
            self.clock.now(),
        )?;
        let items = rows
            .iter()
            .take(limit as usize)
            .map(member_view)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(AdminPage {
            items,
            next_cursor: next,
        })
    }
    pub async fn clients(
        &self,
        context: AdminContext,
        limit: u32,
        cursor: Option<&str>,
        key: &[u8; 32],
    ) -> Result<AdminPage<ClientView>, AdminError> {
        let mut tx = self.pool.begin().await?;
        let actor = authorize(&mut tx, context.session_hash, self.clock.now()).await?;
        let position = page_position(
            limit,
            cursor,
            key,
            actor.user_id,
            "admin/clients",
            self.clock.now(),
        )?;
        let rows=sqlx::query("SELECT id,created_at FROM oauth_clients WHERE ($1::timestamptz IS NULL OR (created_at,id)<($1,$2::uuid)) ORDER BY created_at DESC,id DESC LIMIT $3").bind(position.as_ref().map(|p|p.created_at)).bind(position.as_ref().map(|p|p.id)).bind(i64::from(limit)+1).fetch_all(&mut *tx).await?;
        let next = page_cursor(
            &rows,
            limit,
            key,
            actor.user_id,
            "admin/clients",
            self.clock.now(),
        )?;
        let mut items = Vec::new();
        for row in rows.iter().take(limit as usize) {
            items.push(client_view(&mut tx, row.try_get("id")?).await?);
        }
        Ok(AdminPage {
            items,
            next_cursor: next,
        })
    }
    pub async fn audit_events(
        &self,
        context: AdminContext,
        limit: u32,
        cursor: Option<&str>,
        key: &[u8; 32],
    ) -> Result<AdminPage<AuditView>, AdminError> {
        self.filtered_audit_events(context, limit, cursor, key, &AuditWindow::default())
            .await
    }
    pub async fn filtered_audit_events(
        &self,
        context: AdminContext,
        limit: u32,
        cursor: Option<&str>,
        key: &[u8; 32],
        window: &AuditWindow,
    ) -> Result<AdminPage<AuditView>, AdminError> {
        match (window.from, window.to) {
            (Some(from), Some(to)) if from < to && to - from <= Duration::days(31) => {}
            (None, None) => {}
            _ => return Err(AdminError::InvalidInput),
        }
        let route = filter_route("admin/audit", &(window.from, window.to))?;
        let mut tx = self.pool.begin().await?;
        let actor = authorize(&mut tx, context.session_hash, self.clock.now()).await?;
        let position = page_position(limit, cursor, key, actor.user_id, &route, self.clock.now())?;
        let rows=sqlx::query("SELECT id,event,actor_id,target_type,target_id,result,request_id,source,occurred_at,occurred_at AS created_at FROM audit_events WHERE ($1::timestamptz IS NULL OR (occurred_at,id)<($1,$2::uuid)) AND ($4::timestamptz IS NULL OR occurred_at >= $4) AND ($5::timestamptz IS NULL OR occurred_at < $5) ORDER BY occurred_at DESC,id DESC LIMIT $3").bind(position.as_ref().map(|p|p.created_at)).bind(position.as_ref().map(|p|p.id)).bind(i64::from(limit)+1).bind(window.from).bind(window.to).fetch_all(&mut *tx).await?;
        let next = page_cursor(&rows, limit, key, actor.user_id, &route, self.clock.now())?;
        let mut items = Vec::new();
        for row in rows.iter().take(limit as usize) {
            items.push(AuditView {
                id: row.try_get("id")?,
                event: row.try_get("event")?,
                actor_user_id: row.try_get("actor_id")?,
                target_type: row.try_get("target_type")?,
                target_id: row.try_get("target_id")?,
                result: row.try_get("result")?,
                request_id: row.try_get("request_id")?,
                source: row.try_get("source")?,
                occurred_at: timestamp(row.try_get("occurred_at")?)?,
            });
        }
        Ok(AdminPage {
            items,
            next_cursor: next,
        })
    }
    async fn begin(
        &self,
        context: AdminContext,
        targets: &[Uuid],
    ) -> Result<(Transaction<'_, Postgres>, AdminIdentity), AdminError> {
        let mut tx = self.pool.begin().await?;
        guard(&mut tx).await?;
        let actor: Option<Uuid> =
            sqlx::query_scalar("SELECT user_id FROM sessions WHERE token_hash=$1")
                .bind(context.session_hash.as_bytes())
                .fetch_optional(&mut *tx)
                .await?;
        let actor = actor.ok_or(AdminError::Forbidden)?;
        let mut ids = targets.to_vec();
        ids.push(actor);
        ids.sort();
        ids.dedup();
        sqlx::query("SELECT id FROM users WHERE id=ANY($1::uuid[]) ORDER BY id FOR UPDATE")
            .bind(&ids)
            .fetch_all(&mut *tx)
            .await?;
        let auth = authorize(&mut tx, context.session_hash, self.clock.now()).await?;
        Ok((tx, auth))
    }
}
pub(crate) async fn guard(tx: &mut Transaction<'_, Postgres>) -> Result<(), AdminError> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(ADMIN_LOCK)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
pub(crate) async fn protect_factor_removal(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
    totp: bool,
    credential: Option<Uuid>,
) -> Result<(), AdminError> {
    let admin:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM admin_memberships a JOIN users u ON u.id=a.user_id WHERE a.user_id=$1 AND a.enabled AND u.verified AND u.status='active')").bind(user).fetch_one(&mut **tx).await?;
    if !admin {
        return Ok(());
    }
    let remains:bool=sqlx::query_scalar("SELECT (NOT $2 AND EXISTS(SELECT 1 FROM totp_factors WHERE user_id=$1 AND confirmed)) OR EXISTS(SELECT 1 FROM webauthn_credentials WHERE user_id=$1 AND ($3::uuid IS NULL OR id<>$3))").bind(user).bind(totp).bind(credential).fetch_one(&mut **tx).await?;
    if remains {
        return Ok(());
    }
    protect_last(tx, user).await
}
async fn authorize(
    tx: &mut Transaction<'_, Postgres>,
    hash: Digest,
    now: OffsetDateTime,
) -> Result<AdminIdentity, AdminError> {
    let row=sqlx::query("SELECT s.id,s.user_id,s.strong_at FROM sessions s JOIN users u ON u.id=s.user_id JOIN admin_memberships a ON a.user_id=u.id WHERE s.token_hash=$1 AND s.revoked_at IS NULL AND s.expires_at>$2 AND s.credential_version=u.credential_version AND u.verified AND u.status='active' AND a.enabled").bind(hash.as_bytes()).bind(now).fetch_optional(&mut **tx).await?.ok_or(AdminError::Forbidden)?;
    let user = row.try_get("user_id")?;
    if !has_factor(tx, user).await? {
        return Err(AdminError::Forbidden);
    }
    if row
        .try_get::<Option<OffsetDateTime>, _>("strong_at")?
        .is_none_or(|at| at > now || at + Duration::minutes(5) <= now)
    {
        return Err(AdminError::StrongRequired);
    }
    Ok(AdminIdentity {
        user_id: user,
        session_id: row.try_get("id")?,
    })
}
async fn has_factor(tx: &mut Transaction<'_, Postgres>, user: Uuid) -> Result<bool, AdminError> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM totp_factors WHERE user_id=$1 AND confirmed) OR EXISTS(SELECT 1 FROM webauthn_credentials WHERE user_id=$1)").bind(user).fetch_one(&mut **tx).await?)
}
async fn protect_last(tx: &mut Transaction<'_, Postgres>, target: Uuid) -> Result<(), AdminError> {
    let enabled: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM admin_memberships WHERE user_id=$1 AND enabled)",
    )
    .bind(target)
    .fetch_one(&mut **tx)
    .await?;
    if !enabled {
        return Ok(());
    }
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM admin_memberships a JOIN users u ON u.id=a.user_id WHERE a.enabled AND u.verified AND u.status='active' AND (EXISTS(SELECT 1 FROM totp_factors WHERE user_id=u.id AND confirmed) OR EXISTS(SELECT 1 FROM webauthn_credentials WHERE user_id=u.id)) AND u.id<>$1").bind(target).fetch_one(&mut **tx).await?;
    if count == 0 {
        return Err(AdminError::LastAdministrator);
    }
    Ok(())
}
fn validate_client(
    name: &str,
    scopes: &[Scope],
    redirects: &[String],
    logout: &[String],
    production: bool,
) -> Result<(), AdminError> {
    if name.trim().is_empty()
        || name.chars().count() > 100
        || name.chars().any(char::is_control)
        || scopes.is_empty()
        || !scopes.contains(&Scope::OpenId)
        || redirects.is_empty()
        || redirects.len() > 20
        || logout.len() > 20
    {
        return Err(AdminError::InvalidInput);
    }
    for uris in [redirects, logout] {
        let mut seen = BTreeSet::new();
        for uri in uris {
            validate_redirect_uri(uri, production).map_err(|_| AdminError::InvalidInput)?;
            if !seen.insert(uri) {
                return Err(AdminError::InvalidInput);
            }
        }
    }
    Ok(())
}
fn scope_strings(scopes: &[Scope]) -> Vec<String> {
    scopes.iter().map(|scope| scope.as_str().into()).collect()
}
fn parse_scopes(scopes: &[String]) -> Result<Vec<Scope>, AdminError> {
    scopes
        .iter()
        .map(|scope| match scope.as_str() {
            "openid" => Ok(Scope::OpenId),
            "profile" => Ok(Scope::Profile),
            "email" => Ok(Scope::Email),
            _ => Err(AdminError::InvalidInput),
        })
        .collect()
}
async fn replace_uris(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    redirects: &[String],
    logout: &[String],
) -> Result<(), AdminError> {
    sqlx::query("DELETE FROM oauth_redirect_uris WHERE client_id=$1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    for (kind, uris) in [("login", redirects), ("logout", logout)] {
        for uri in uris {
            sqlx::query(
                "INSERT INTO oauth_redirect_uris(id,client_id,uri,kind) VALUES($1,$2,$3,$4)",
            )
            .bind(Uuid::new_v4())
            .bind(id)
            .bind(uri)
            .bind(kind)
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(())
}
async fn client_view(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<ClientView, AdminError> {
    let row = sqlx::query(
        "SELECT id,client_id,name,enabled,allowed_scopes,created_at FROM oauth_clients WHERE id=$1",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(AdminError::NotFound)?;
    let uris = sqlx::query(
        "SELECT kind,uri FROM oauth_redirect_uris WHERE client_id=$1 ORDER BY kind,uri",
    )
    .bind(id)
    .fetch_all(&mut **tx)
    .await?;
    let mut redirects = Vec::new();
    let mut logout = Vec::new();
    for uri in uris {
        if uri.try_get::<String, _>("kind")? == "login" {
            redirects.push(uri.try_get("uri")?);
        } else {
            logout.push(uri.try_get("uri")?);
        }
    }
    Ok(ClientView {
        id,
        client_id: row.try_get("client_id")?,
        name: row.try_get("name")?,
        enabled: row.try_get("enabled")?,
        allowed_scopes: row.try_get("allowed_scopes")?,
        created_at: timestamp(row.try_get("created_at")?)?,
        redirect_uris: redirects,
        post_logout_redirect_uris: logout,
    })
}
fn member_view(row: &PgRow) -> Result<MemberView, AdminError> {
    Ok(MemberView {
        user_id: row.try_get("user_id")?,
        email: row.try_get("email")?,
        enabled: row.try_get("enabled")?,
        created_at: timestamp(row.try_get("created_at")?)?,
    })
}
fn timestamp(time: OffsetDateTime) -> Result<String, AdminError> {
    time.format(&Rfc3339).map_err(|_| AdminError::Unavailable)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PagePosition {
    actor: Uuid,
    route: String,
    id: Uuid,
    created_at: OffsetDateTime,
    expires_at: OffsetDateTime,
}
fn filter_route<T: Serialize>(route: &str, filters: &T) -> Result<String, AdminError> {
    use base64::Engine;
    let serialized = serde_json::to_string(filters).map_err(|_| AdminError::InvalidInput)?;
    Ok(format!(
        "{route}/{}",
        base64::prelude::BASE64_URL_SAFE_NO_PAD.encode(token_digest(&serialized))
    ))
}
fn page_position(
    limit: u32,
    cursor: Option<&str>,
    key: &[u8; 32],
    actor: Uuid,
    route: &str,
    now: OffsetDateTime,
) -> Result<Option<PagePosition>, AdminError> {
    use base64::Engine;
    if !(1..=100).contains(&limit) {
        return Err(AdminError::InvalidInput);
    }
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    if cursor.len() > 2048 {
        return Err(AdminError::InvalidInput);
    }
    let (payload, mac) = cursor.split_once('.').ok_or(AdminError::InvalidInput)?;
    let payload = base64::prelude::BASE64_URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| AdminError::InvalidInput)?;
    let text = std::str::from_utf8(&payload).map_err(|_| AdminError::InvalidInput)?;
    let mac = base64::prelude::BASE64_URL_SAFE_NO_PAD
        .decode(mac)
        .map_err(|_| AdminError::InvalidInput)?;
    if !identity_core::security::constant_time_equal(
        &mac,
        &identity_core::security::keyed_account_digest(key, "admin-cursor-v1", text),
    ) {
        return Err(AdminError::InvalidInput);
    }
    let parsed: PagePosition = serde_json::from_str(text).map_err(|_| AdminError::InvalidInput)?;
    if parsed.actor != actor || parsed.route != route || parsed.expires_at <= now {
        return Err(AdminError::InvalidInput);
    }
    Ok(Some(parsed))
}
fn page_cursor(
    rows: &[PgRow],
    limit: u32,
    key: &[u8; 32],
    actor: Uuid,
    route: &str,
    now: OffsetDateTime,
) -> Result<Option<String>, AdminError> {
    use base64::Engine;
    if rows.len() <= limit as usize {
        return Ok(None);
    }
    let row = &rows[limit as usize - 1];
    let payload = PagePosition {
        actor,
        route: route.into(),
        id: row.try_get("id")?,
        created_at: row.try_get("created_at")?,
        expires_at: now + Duration::minutes(10),
    };
    let text = serde_json::to_string(&payload).map_err(|_| AdminError::InvalidInput)?;
    let mac = identity_core::security::keyed_account_digest(key, "admin-cursor-v1", &text);
    Ok(Some(format!(
        "{}.{}",
        base64::prelude::BASE64_URL_SAFE_NO_PAD.encode(text),
        base64::prelude::BASE64_URL_SAFE_NO_PAD.encode(mac)
    )))
}
struct AuditContext {
    request_id: Uuid,
    source: Digest,
    now: OffsetDateTime,
}
async fn denied_last(
    tx: &mut Transaction<'_, Postgres>,
    actor: Uuid,
    target: Uuid,
    context: AdminContext,
    now: OffsetDateTime,
) -> Result<(), AdminError> {
    insert_audit(
        tx,
        &AuditRecord {
            id: Uuid::new_v4(),
            event: AuditEvent::LastAdminRemovalDenied,
            actor_id: Some(actor),
            target: AuditTarget::AdminMember,
            target_id: Some(target),
            result: AuditResult::Denied,
            request_id: context.request_id,
            source_hash: context.source_hash,
            occurred_at: now,
        },
    )
    .await
    .map_err(|_| AdminError::Unavailable)
}
async fn audit_ctx(
    tx: &mut Transaction<'_, Postgres>,
    actor: Uuid,
    event: AuditEvent,
    target: AuditTarget,
    id: Uuid,
    ctx: AdminContext,
    now: OffsetDateTime,
) -> Result<(), AdminError> {
    audit(
        tx,
        actor,
        event,
        target,
        id,
        AuditContext {
            request_id: ctx.request_id,
            source: ctx.source_hash,
            now,
        },
    )
    .await
}
async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    actor: Uuid,
    event: AuditEvent,
    target: AuditTarget,
    id: Uuid,
    context: AuditContext,
) -> Result<(), AdminError> {
    insert_audit(
        tx,
        &AuditRecord {
            id: Uuid::new_v4(),
            event,
            actor_id: Some(actor),
            target,
            target_id: Some(id),
            result: AuditResult::Success,
            request_id: context.request_id,
            source_hash: context.source,
            occurred_at: context.now,
        },
    )
    .await
    .map_err(|_| AdminError::Unavailable)
}
