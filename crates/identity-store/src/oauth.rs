//! Managed clients and single-use browser-bound authorization transactions.
use crate::{
    repository::Digest,
    security::{AuditEvent, AuditRecord, AuditResult, AuditTarget, insert_audit},
};
use identity_core::{
    clock::Clock,
    oauth::{
        AuthorizationRequest, Prompt, Scope, valid_challenge, valid_state, validate_redirect_uri,
    },
    security::{Token, token_digest},
};
use serde::Serialize;
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use std::{collections::BTreeSet, fmt, sync::Arc};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use url::Url;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthStoreError {
    InvalidClient,
    InvalidRedirect,
    InvalidScope,
    InvalidContext,
    NotFound,
    Consumed,
    Expired,
    LoginRequired,
    ConsentRequired,
    Unavailable,
}
impl fmt::Display for OAuthStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidClient => "unauthorized_client",
            Self::InvalidRedirect | Self::InvalidContext => "invalid_request",
            Self::InvalidScope => "invalid_scope",
            Self::NotFound => "transaction not found",
            Self::Consumed => "transaction consumed",
            Self::Expired => "transaction expired",
            Self::LoginRequired => "login_required",
            Self::ConsentRequired => "consent_required",
            Self::Unavailable => "temporarily_unavailable",
        })
    }
}
impl std::error::Error for OAuthStoreError {}
impl From<sqlx::Error> for OAuthStoreError {
    fn from(_: sqlx::Error) -> Self {
        Self::Unavailable
    }
}
#[derive(Clone, Copy)]
pub struct BrowserBinding {
    pub session_hash: Option<Digest>,
    pub preauth_hash: Option<Digest>,
}
#[derive(Clone, Copy)]
pub struct OAuthAudit {
    pub request_id: Uuid,
    pub source_hash: Digest,
}
pub struct NewClient {
    pub client_id: String,
    pub name: String,
    pub allowed_scopes: Vec<Scope>,
    pub redirect_uris: Vec<String>,
    pub logout_uris: Vec<String>,
    pub production: bool,
}
pub struct CreatedClient {
    pub id: Uuid,
    pub client_id: String,
    pub client_secret: Token,
}
pub enum PrepareOutcome {
    NeedLogin {
        transaction_id: Uuid,
    },
    Consent {
        transaction_id: Uuid,
    },
    ReadyCode(CodeOutput),
    ProtocolError {
        error: &'static str,
        redirect_to: String,
    },
}
pub struct CodeOutput {
    pub code: Token,
    pub grant_id: Uuid,
    pub redirect_uri: String,
    pub state: String,
}
impl CodeOutput {
    pub fn redirect_to(&self) -> Result<String, OAuthStoreError> {
        redirect(&self.redirect_uri, "code", self.code.expose(), &self.state)
    }
}
#[derive(Serialize)]
pub struct ClientView {
    pub client_id: String,
    pub name: String,
}
#[derive(Serialize)]
pub struct TransactionView {
    pub id: Uuid,
    pub client: ClientView,
    pub requested_scopes: Vec<String>,
    pub previously_approved_scopes: Vec<String>,
    pub expires_at: String,
    pub status: &'static str,
}
#[derive(Clone)]
pub struct OAuthStore {
    pool: PgPool,
    clock: Arc<dyn Clock>,
}
impl OAuthStore {
    pub fn new(pool: PgPool, clock: Arc<dyn Clock>) -> Self {
        Self { pool, clock }
    }
    pub async fn validate_callback(
        &self,
        client_id: &str,
        redirect_uri: &str,
    ) -> Result<bool, OAuthStoreError> {
        Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM oauth_clients c JOIN oauth_redirect_uris r ON r.client_id=c.id WHERE c.client_id=$1 AND c.enabled AND r.kind='login' AND r.uri=$2)").bind(client_id).bind(redirect_uri).fetch_one(&self.pool).await?)
    }
    pub async fn valid_preauth(&self, hash: Digest) -> Result<bool, OAuthStoreError> {
        Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM preauthentication_contexts WHERE token_hash=$1 AND revoked_at IS NULL AND expires_at>$2)").bind(hash.as_bytes()).bind(self.clock.now()).fetch_one(&self.pool).await?)
    }
    pub async fn create_client(&self, input: &NewClient) -> Result<CreatedClient, OAuthStoreError> {
        if input.client_id.is_empty()
            || input.client_id.len() > 128
            || !input.client_id.is_ascii()
            || input
                .client_id
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
            || input.name.trim().is_empty()
            || input.name.chars().count() > 100
            || input.name.chars().any(char::is_control)
            || input.allowed_scopes.is_empty()
            || !input.allowed_scopes.contains(&Scope::OpenId)
            || input.redirect_uris.is_empty()
            || input.redirect_uris.len() > 20
            || input.logout_uris.len() > 20
        {
            return Err(OAuthStoreError::InvalidClient);
        }
        for uris in [&input.redirect_uris, &input.logout_uris] {
            let mut seen = BTreeSet::new();
            for uri in uris {
                validate_redirect_uri(uri, input.production)
                    .map_err(|_| OAuthStoreError::InvalidRedirect)?;
                if !seen.insert(uri) {
                    return Err(OAuthStoreError::InvalidRedirect);
                }
            }
        }
        let secret = Token::generate().map_err(|_| OAuthStoreError::Unavailable)?;
        let id = Uuid::new_v4();
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO oauth_clients(id,client_id,secret_hash,name,allowed_scopes,created_at,updated_at) VALUES($1,$2,$3,$4,$5,$6,$6)").bind(id).bind(&input.client_id).bind(token_digest(secret.expose()).as_slice()).bind(&input.name).bind(scopes(&input.allowed_scopes)).bind(now).execute(&mut *tx).await?;
        for (kind, uris) in [
            ("login", &input.redirect_uris),
            ("logout", &input.logout_uris),
        ] {
            for uri in uris {
                sqlx::query(
                    "INSERT INTO oauth_redirect_uris(id,client_id,uri,kind) VALUES($1,$2,$3,$4)",
                )
                .bind(Uuid::new_v4())
                .bind(id)
                .bind(uri)
                .bind(kind)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
        Ok(CreatedClient {
            id,
            client_id: input.client_id.clone(),
            client_secret: secret,
        })
    }
    pub async fn prepare(
        &self,
        request: &AuthorizationRequest,
        binding: BrowserBinding,
        audit: OAuthAudit,
    ) -> Result<PrepareOutcome, OAuthStoreError> {
        if request.scopes.is_empty()
            || !request.scopes.contains(&Scope::OpenId)
            || request
                .scopes
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len()
                != request.scopes.len()
        {
            return Err(OAuthStoreError::InvalidScope);
        }
        if !valid_state(&request.state)
            || !valid_state(&request.nonce)
            || !valid_challenge(&request.code_challenge)
            || request.max_age.is_some_and(|value| value > i64::MAX as u64)
        {
            return Err(OAuthStoreError::InvalidContext);
        }
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let client = sqlx::query(
            "SELECT id,allowed_scopes FROM oauth_clients WHERE client_id=$1 AND enabled",
        )
        .bind(&request.client_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(OAuthStoreError::InvalidClient)?;
        let client_id: Uuid = client.try_get("id")?;
        let registered:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM oauth_redirect_uris WHERE client_id=$1 AND uri=$2 AND kind='login')").bind(client_id).bind(&request.redirect_uri).fetch_one(&mut *tx).await?;
        if !registered {
            return Err(OAuthStoreError::InvalidRedirect);
        }
        let requested = scopes(&request.scopes);
        let allowed: Vec<String> = client.try_get("allowed_scopes")?;
        if requested.iter().any(|scope| !allowed.contains(scope)) {
            return Ok(PrepareOutcome::ProtocolError {
                error: "invalid_scope",
                redirect_to: redirect(
                    &request.redirect_uri,
                    "error",
                    "invalid_scope",
                    &request.state,
                )?,
            });
        }
        let session = valid_session(&mut tx, binding.session_hash, now).await?;
        let mut approved = Vec::new();
        if let Some(session) = &session {
            approved = sqlx::query_scalar(
                "SELECT scopes FROM user_consents WHERE user_id=$1 AND client_id=$2",
            )
            .bind(session.user)
            .bind(client_id)
            .fetch_optional(&mut *tx)
            .await?
            .unwrap_or_default();
        }
        let needs_login = session.is_none()
            || request.prompt.contains(&Prompt::Login)
            || request.max_age.is_some_and(|max| {
                session.as_ref().is_some_and(|session| {
                    (now - session.auth_time).whole_seconds()
                        > i64::try_from(max).unwrap_or(i64::MAX)
                })
            });
        let consent = request.prompt.contains(&Prompt::Consent)
            || requested.iter().any(|scope| !approved.contains(scope));
        if request.prompt.contains(&Prompt::None) && (needs_login || consent) {
            let error = if needs_login {
                "login_required"
            } else {
                "consent_required"
            };
            return Ok(PrepareOutcome::ProtocolError {
                error,
                redirect_to: redirect(&request.redirect_uri, "error", error, &request.state)?,
            });
        }
        let preauth = if session.is_none() {
            let hash = binding
                .preauth_hash
                .ok_or(OAuthStoreError::InvalidContext)?;
            let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM preauthentication_contexts WHERE token_hash=$1 AND revoked_at IS NULL AND expires_at>$2)").bind(hash.as_bytes()).bind(now).fetch_one(&mut *tx).await?;
            if !valid {
                return Err(OAuthStoreError::InvalidContext);
            }
            Some(hash)
        } else {
            None
        };
        let id = Uuid::new_v4();
        let prompt: Vec<String> = request
            .prompt
            .iter()
            .map(|p| {
                match p {
                    Prompt::Login => "login",
                    Prompt::Consent => "consent",
                    Prompt::None => "none",
                }
                .into()
            })
            .collect();
        let previous = if request.prompt.contains(&Prompt::Login) {
            session.as_ref().map(|s| s.auth_time)
        } else {
            None
        };
        sqlx::query("INSERT INTO authorization_transactions(id,client_id,preauth_hash,session_id,redirect_uri,scopes,state,nonce,code_challenge,code_challenge_method,prompt,max_age,previous_auth_time,created_at,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,'S256',$10,$11,$12,$13,$14)").bind(id).bind(client_id).bind(preauth.as_ref().map(Digest::as_bytes)).bind(session.as_ref().map(|s|s.id)).bind(&request.redirect_uri).bind(&requested).bind(&request.state).bind(&request.nonce).bind(&request.code_challenge).bind(prompt).bind(request.max_age.map(i64::try_from).transpose().map_err(|_|OAuthStoreError::InvalidContext)?).bind(previous).bind(now).bind(now+Duration::minutes(5)).execute(&mut *tx).await?;
        tx.commit().await?;
        if needs_login {
            Ok(PrepareOutcome::NeedLogin { transaction_id: id })
        } else if consent {
            Ok(PrepareOutcome::Consent { transaction_id: id })
        } else {
            self.decision(id, binding, true, audit)
                .await
                .map(PrepareOutcome::ReadyCode)
        }
    }
    pub async fn view(
        &self,
        id: Uuid,
        binding: BrowserBinding,
    ) -> Result<TransactionView, OAuthStoreError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let row = owned_transaction(&mut tx, id, binding, now, false).await?;
        validate_transaction(&row, now)?;
        let session = valid_session(&mut tx, binding.session_hash, now).await?;
        let approved = if let Some(session) = &session {
            sqlx::query_scalar("SELECT scopes FROM user_consents WHERE user_id=$1 AND client_id=$2")
                .bind(session.user)
                .bind(row.try_get::<Uuid, _>("client_id")?)
                .fetch_optional(&mut *tx)
                .await?
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let status = if session.is_none() || needs_reauthentication(&row, session.as_ref(), now)? {
            "login_required"
        } else {
            "consent_required"
        };
        Ok(TransactionView {
            id,
            client: ClientView {
                client_id: row.try_get("public_client_id")?,
                name: row.try_get("client_name")?,
            },
            requested_scopes: row.try_get("scopes")?,
            previously_approved_scopes: approved,
            expires_at: row
                .try_get::<OffsetDateTime, _>("expires_at")?
                .format(&Rfc3339)
                .map_err(|_| OAuthStoreError::Unavailable)?,
            status,
        })
    }
    pub async fn decision(
        &self,
        id: Uuid,
        binding: BrowserBinding,
        approve: bool,
        audit: OAuthAudit,
    ) -> Result<CodeOutput, OAuthStoreError> {
        let now = self.clock.now();
        let located=sqlx::query("SELECT s.user_id FROM authorization_transactions t JOIN sessions s ON s.id=t.session_id WHERE t.id=$1").bind(id).fetch_optional(&self.pool).await?.ok_or(OAuthStoreError::LoginRequired)?;
        let user: Uuid = located.try_get("user_id")?;
        let mut tx = self.pool.begin().await?;
        let status = sqlx::query(
            "SELECT verified,status,credential_version FROM users WHERE id=$1 FOR UPDATE",
        )
        .bind(user)
        .fetch_one(&mut *tx)
        .await?;
        if !status.try_get::<bool, _>("verified")?
            || status.try_get::<String, _>("status")? != "active"
        {
            return Err(OAuthStoreError::LoginRequired);
        }
        let session = valid_session(&mut tx, binding.session_hash, now)
            .await?
            .ok_or(OAuthStoreError::LoginRequired)?;
        if session.user != user {
            return Err(OAuthStoreError::InvalidContext);
        }
        sqlx::query("SELECT id FROM sessions WHERE id=$1 FOR UPDATE")
            .bind(session.id)
            .fetch_one(&mut *tx)
            .await?;
        let row = owned_transaction(&mut tx, id, binding, now, true).await?;
        validate_transaction(&row, now)?;
        if needs_reauthentication(&row, Some(&session), now)? {
            return Err(OAuthStoreError::LoginRequired);
        }
        let client_id: Uuid = row.try_get("client_id")?;
        // Keep client enabled through this commit. Client-disable code must not hold client then acquire users.
        let enabled: bool =
            sqlx::query_scalar("SELECT enabled FROM oauth_clients WHERE id=$1 FOR SHARE")
                .bind(client_id)
                .fetch_one(&mut *tx)
                .await?;
        if !enabled {
            return Err(OAuthStoreError::InvalidClient);
        }
        if !approve {
            return Err(OAuthStoreError::InvalidContext);
        }
        let grant_id = Uuid::new_v4();
        let code = Token::generate().map_err(|_| OAuthStoreError::Unavailable)?;
        let requested: Vec<String> = row.try_get("scopes")?;
        let allowed: Vec<String> =
            sqlx::query_scalar("SELECT allowed_scopes FROM oauth_clients WHERE id=$1")
                .bind(client_id)
                .fetch_one(&mut *tx)
                .await?;
        if requested.iter().any(|scope| !allowed.contains(scope)) {
            return Err(OAuthStoreError::InvalidScope);
        }
        sqlx::query("INSERT INTO oauth_grants(id,user_id,client_id,session_id,scopes,expires_at,created_at) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(grant_id).bind(user).bind(client_id).bind(session.id).bind(&requested).bind(session.expires_at).bind(now).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO authorization_codes(id,code_hash,grant_id,redirect_uri,code_challenge,code_challenge_method,nonce,created_at,expires_at) VALUES($1,$2,$3,$4,$5,'S256',$6,$7,$8)").bind(Uuid::new_v4()).bind(token_digest(code.expose()).as_slice()).bind(grant_id).bind(row.try_get::<String,_>("redirect_uri")?).bind(row.try_get::<String,_>("code_challenge")?).bind(row.try_get::<String,_>("nonce")?).bind(now).bind(now+Duration::seconds(60)).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO user_consents(id,user_id,client_id,scopes,created_at,updated_at) VALUES($1,$2,$3,$4,$5,$5) ON CONFLICT(user_id,client_id) DO UPDATE SET scopes=ARRAY(SELECT DISTINCT unnest(user_consents.scopes||EXCLUDED.scopes) ORDER BY 1),updated_at=EXCLUDED.updated_at").bind(Uuid::new_v4()).bind(user).bind(client_id).bind(&requested).bind(now).execute(&mut *tx).await?;
        sqlx::query("UPDATE authorization_transactions SET consumed_at=$2 WHERE id=$1")
            .bind(id)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        insert_audit(
            &mut tx,
            &AuditRecord {
                id: Uuid::new_v4(),
                event: AuditEvent::ConsentApproved,
                actor_id: Some(user),
                target: AuditTarget::Grant,
                target_id: Some(grant_id),
                result: AuditResult::Success,
                request_id: audit.request_id,
                source_hash: audit.source_hash,
                occurred_at: now,
            },
        )
        .await
        .map_err(|_| OAuthStoreError::Unavailable)?;
        let result = CodeOutput {
            code,
            grant_id,
            redirect_uri: row.try_get("redirect_uri")?,
            state: row.try_get("state")?,
        };
        tx.commit().await?;
        Ok(result)
    }
    pub async fn deny(
        &self,
        id: Uuid,
        binding: BrowserBinding,
        audit: OAuthAudit,
    ) -> Result<String, OAuthStoreError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let initial = valid_session(&mut tx, binding.session_hash, now)
            .await?
            .ok_or(OAuthStoreError::LoginRequired)?;
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(initial.user)
            .fetch_one(&mut *tx)
            .await?;
        sqlx::query("SELECT id FROM sessions WHERE id=$1 FOR UPDATE")
            .bind(initial.id)
            .fetch_one(&mut *tx)
            .await?;
        let session = valid_session(&mut tx, binding.session_hash, now)
            .await?
            .ok_or(OAuthStoreError::LoginRequired)?;
        let row = owned_transaction(&mut tx, id, binding, now, true).await?;
        validate_transaction(&row, now)?;
        if needs_reauthentication(&row, Some(&session), now)? {
            return Err(OAuthStoreError::LoginRequired);
        }
        sqlx::query("UPDATE authorization_transactions SET consumed_at=$2 WHERE id=$1")
            .bind(id)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        insert_audit(
            &mut tx,
            &AuditRecord {
                id: Uuid::new_v4(),
                event: AuditEvent::ConsentDenied,
                actor_id: Some(session.user),
                target: AuditTarget::Grant,
                target_id: None,
                result: AuditResult::Denied,
                request_id: audit.request_id,
                source_hash: audit.source_hash,
                occurred_at: now,
            },
        )
        .await
        .map_err(|_| OAuthStoreError::Unavailable)?;
        let result = redirect(
            &row.try_get::<String, _>("redirect_uri")?,
            "error",
            "access_denied",
            &row.try_get::<String, _>("state")?,
        )?;
        tx.commit().await?;
        Ok(result)
    }
}
fn scopes(scopes: &[Scope]) -> Vec<String> {
    scopes.iter().map(|scope| scope.as_str().into()).collect()
}
struct SessionAuthority {
    id: Uuid,
    user: Uuid,
    auth_time: OffsetDateTime,
    expires_at: OffsetDateTime,
}
async fn valid_session(
    tx: &mut Transaction<'_, Postgres>,
    hash: Option<Digest>,
    now: OffsetDateTime,
) -> Result<Option<SessionAuthority>, OAuthStoreError> {
    let Some(hash) = hash else { return Ok(None) };
    let row=sqlx::query("SELECT s.id,s.user_id,s.auth_time,s.expires_at FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=$1 AND s.revoked_at IS NULL AND s.expires_at>$2 AND s.credential_version=u.credential_version AND u.verified AND u.status='active'").bind(hash.as_bytes()).bind(now).fetch_optional(&mut **tx).await?;
    row.map(|row| {
        Ok(SessionAuthority {
            id: row.try_get("id")?,
            user: row.try_get("user_id")?,
            auth_time: row.try_get("auth_time")?,
            expires_at: row.try_get("expires_at")?,
        })
    })
    .transpose()
}
async fn owned_transaction(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    binding: BrowserBinding,
    now: OffsetDateTime,
    lock: bool,
) -> Result<PgRow, OAuthStoreError> {
    let session = valid_session(tx, binding.session_hash, now).await?;
    let preauth = if let Some(hash) = binding.preauth_hash {
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM preauthentication_contexts WHERE token_hash=$1 AND revoked_at IS NULL AND expires_at>$2)").bind(hash.as_bytes()).bind(now).fetch_one(&mut **tx).await?;
        if valid { Some(hash) } else { None }
    } else {
        None
    };
    let sql = if lock {
        "SELECT t.*,c.client_id AS public_client_id,c.name AS client_name FROM authorization_transactions t JOIN oauth_clients c ON c.id=t.client_id WHERE t.id=$1 AND c.enabled AND ((t.session_id=$2 AND t.preauth_hash IS NULL) OR (t.preauth_hash=$3 AND t.session_id IS NULL)) FOR UPDATE OF t"
    } else {
        "SELECT t.*,c.client_id AS public_client_id,c.name AS client_name FROM authorization_transactions t JOIN oauth_clients c ON c.id=t.client_id WHERE t.id=$1 AND c.enabled AND ((t.session_id=$2 AND t.preauth_hash IS NULL) OR (t.preauth_hash=$3 AND t.session_id IS NULL))"
    };
    sqlx::query(sql)
        .bind(id)
        .bind(session.map(|s| s.id))
        .bind(preauth.as_ref().map(Digest::as_bytes))
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(OAuthStoreError::NotFound)
}
fn validate_transaction(row: &PgRow, now: OffsetDateTime) -> Result<(), OAuthStoreError> {
    if row
        .try_get::<Option<OffsetDateTime>, _>("consumed_at")?
        .is_some()
    {
        return Err(OAuthStoreError::Consumed);
    }
    if row.try_get::<OffsetDateTime, _>("expires_at")? <= now {
        return Err(OAuthStoreError::Expired);
    }
    Ok(())
}
fn needs_reauthentication(
    row: &PgRow,
    session: Option<&SessionAuthority>,
    now: OffsetDateTime,
) -> Result<bool, OAuthStoreError> {
    let Some(session) = session else {
        return Ok(true);
    };
    let previous: Option<OffsetDateTime> = row.try_get("previous_auth_time")?;
    let max: Option<i64> = row.try_get("max_age")?;
    Ok(
        previous.is_some_and(|previous| session.auth_time <= previous)
            || max.is_some_and(|max| (now - session.auth_time).whole_seconds() > max),
    )
}
fn redirect(uri: &str, name: &str, value: &str, state: &str) -> Result<String, OAuthStoreError> {
    let mut url = Url::parse(uri).map_err(|_| OAuthStoreError::InvalidRedirect)?;
    url.query_pairs_mut()
        .append_pair(name, value)
        .append_pair("state", state);
    Ok(url.into())
}
