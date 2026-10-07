//! Implemented T05 register/email-verification endpoints. They never issue identity sessions.
use crate::security::{
    ApiError, LimitDecision, LimitPolicy, RequestId, SecurityState, TrustedSource, read_json,
    security_router,
};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use identity_core::{
    config::Config,
    security::{
        AeadKeyRing, Password, PasswordService, SecurityError, normalize_email, token_digest,
    },
};
use identity_store::{
    Dependencies,
    accounts::{AccountsStore, ConfirmationError, MailAction, NewRegistration},
    repository::Digest,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

#[derive(Clone)]
pub struct AuthAppState {
    pub security: SecurityState,
    pub(crate) inner: Arc<AccountsInner>,
    pub(crate) sessions: identity_store::sessions::SessionService,
    pub(crate) password_store: identity_store::passwords::PasswordStore,
    pub(crate) mfa: identity_store::mfa::MfaService,
    pub(crate) passkeys: identity_store::passkeys::PasskeyService,
    pub(crate) oauth: identity_store::oauth::OAuthStore,
}
pub(crate) struct AccountsInner {
    store: AccountsStore,
    pub(crate) passwords: PasswordService,
    pub(crate) keys: Arc<AeadKeyRing>,
    pub(crate) issuer: String,
}
impl AuthAppState {
    pub async fn new(config: &Config, dependencies: Dependencies) -> Result<Self, &'static str> {
        Self::new_with_clock(
            config,
            dependencies,
            Arc::new(identity_core::clock::SystemClock),
        )
        .await
    }
    pub async fn new_with_clock(
        config: &Config,
        dependencies: Dependencies,
        clock: Arc<dyn identity_core::clock::Clock>,
    ) -> Result<Self, &'static str> {
        let security = SecurityState::new_with_clock(config, dependencies.clone(), clock.clone())?;
        let keys = Arc::new(
            AeadKeyRing::load_file(&config.encryption_keys_file, &config.active_encryption_kid)
                .map_err(|_| "invalid encryption key configuration")?,
        );
        let passwords = PasswordService::initialize(config.argon2_parallelism_limit)
            .await
            .map_err(|_| "password service unavailable")?;
        Ok(Self {
            security,
            oauth: identity_store::oauth::OAuthStore::new(
                dependencies.postgres.clone(),
                clock.clone(),
            ),
            passkeys: identity_store::passkeys::PasskeyService::new(
                dependencies.postgres.clone(),
                clock.clone(),
                keys.clone(),
                &config.issuer,
                &config.rp_id,
            )
            .map_err(|_| "WebAuthn configuration invalid")?,
            mfa: identity_store::mfa::MfaService::new(
                dependencies.postgres.clone(),
                clock.clone(),
                keys.clone(),
                config.issuer.origin().ascii_serialization(),
            ),
            password_store: identity_store::passwords::PasswordStore::new(
                dependencies.postgres.clone(),
                clock.clone(),
            ),
            sessions: identity_store::sessions::SessionService::new(
                dependencies.postgres.clone(),
                clock.clone(),
            ),
            inner: Arc::new(AccountsInner {
                store: AccountsStore::new(dependencies.postgres),
                passwords,
                keys,
                issuer: config.issuer.origin().ascii_serialization(),
            }),
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterRequest {
    email: String,
    password: String,
}
impl Drop for RegisterRequest {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmailRequest {
    email: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmRequest {
    token: String,
}

pub fn accounts_router(state: AuthAppState, existing: Router) -> Router {
    let accounts = Router::new()
        .route("/api/v1/auth/register", post(register))
        .route(
            "/api/v1/auth/email-verification/request",
            post(request_verification),
        )
        .route(
            "/api/v1/auth/email-verification/confirm",
            post(confirm_verification),
        )
        .with_state(state.clone());
    security_router(
        state.security.clone(),
        existing
            .merge(accounts)
            .merge(crate::sessions::session_routes(state.clone()))
            .merge(crate::passwords::password_routes(state.clone()))
            .merge(crate::mfa::mfa_routes(state.clone()))
            .merge(crate::passkeys::passkey_routes(state.clone()))
            .merge(crate::oauth::oauth_routes(state)),
    )
}

fn context(request: &Request) -> Result<(Uuid, std::net::IpAddr), ApiError> {
    let id = request
        .extensions()
        .get::<RequestId>()
        .map_or_else(Uuid::new_v4, |id| id.0);
    let source = request
        .extensions()
        .get::<TrustedSource>()
        .map(|source| source.0)
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "DEPENDENCY_UNAVAILABLE",
                id,
            )
        })?;
    Ok((id, source))
}
pub(crate) fn accepted() -> Response {
    (StatusCode::ACCEPTED,Json(json!({"status":"accepted","message":"如果该邮箱可接收此操作的邮件，我们将发送后续说明。"}))).into_response()
}
pub(crate) async fn mail_budget(
    state: &AuthAppState,
    source: std::net::IpAddr,
    email: &str,
    id: Uuid,
) -> Result<bool, ApiError> {
    match state
        .security
        .check_limit(LimitPolicy::Mail, source, Some(email), None)
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "DEPENDENCY_UNAVAILABLE",
                id,
            )
        })? {
        LimitDecision::Allowed => Ok(true),
        LimitDecision::Limited {
            mail_target: true, ..
        } => Ok(false),
        LimitDecision::Limited { retry_after, .. } => Err(ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "RATE_LIMITED",
            request_id: id,
            retry_after: Some(retry_after),
        }),
    }
}
fn input_error(id: Uuid) -> ApiError {
    ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "INPUT_INVALID", id)
}
fn source_digest(state: &AuthAppState, source: std::net::IpAddr) -> Digest {
    state.security.source_digest(source)
}

async fn register(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let (id, source) = context(&request)?;
    let mut value = read_json::<RegisterRequest>(request).await?;
    let email = normalize_email(&value.email).map_err(|_| input_error(id))?;
    let raw_password = Zeroizing::new(std::mem::take(&mut value.password));
    let password = Password::new(&raw_password).map_err(|_| input_error(id))?;
    if !mail_budget(&state, source, &email, id).await? {
        return Ok(accepted());
    }
    // Hash even for an existing email: equivalent resource cost and no password replacement.
    let hash = state
        .inner
        .passwords
        .hash(&password)
        .await
        .map_err(|reason| {
            let mut error = ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "DEPENDENCY_UNAVAILABLE",
                id,
            );
            if matches!(reason, SecurityError::Busy) {
                error.retry_after = Some(1);
            }
            error
        })?;
    let now = OffsetDateTime::now_utc();
    let user_id = Uuid::new_v4();
    let mail =
        MailAction::new(user_id, &state.inner.issuer, &state.inner.keys, now).map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "DEPENDENCY_UNAVAILABLE",
                id,
            )
        })?;
    state
        .inner
        .store
        .register(NewRegistration {
            user_id,
            email: &email,
            password_hash: &hash,
            mail: &mail,
            source: source_digest(&state, source),
            request_id: id,
            now,
        })
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "DEPENDENCY_UNAVAILABLE",
                id,
            )
        })?;
    Ok(accepted())
}

async fn request_verification(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let (id, source) = context(&request)?;
    let value = read_json::<EmailRequest>(request).await?;
    let email = normalize_email(&value.email).map_err(|_| input_error(id))?;
    if !mail_budget(&state, source, &email, id).await? {
        return Ok(accepted());
    }
    state
        .inner
        .store
        .request_verification(
            &email,
            &state.inner.issuer,
            &state.inner.keys,
            source_digest(&state, source),
            id,
            OffsetDateTime::now_utc(),
        )
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "DEPENDENCY_UNAVAILABLE",
                id,
            )
        })?;
    Ok(accepted())
}
async fn confirm_verification(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let (id, source) = context(&request)?;
    let value = read_json::<ConfirmRequest>(request).await?;
    let token = Zeroizing::new(value.token);
    if token.len() != 43
        || BASE64_URL_SAFE_NO_PAD
            .decode(token.as_bytes())
            .map_or(true, |bytes| bytes.len() != 32)
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "AUTH_ACTION_INVALID",
            id,
        ));
    }
    match state
        .security
        .check_limit(LimitPolicy::Challenge, source, None, None)
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "DEPENDENCY_UNAVAILABLE",
                id,
            )
        })? {
        LimitDecision::Allowed => {}
        LimitDecision::Limited { retry_after, .. } => {
            return Err(ApiError {
                status: StatusCode::TOO_MANY_REQUESTS,
                code: "RATE_LIMITED",
                request_id: id,
                retry_after: Some(retry_after),
            });
        }
    }
    state
        .inner
        .store
        .confirm_verification(
            Digest::from_bytes(token_digest(&token)),
            source_digest(&state, source),
            id,
            OffsetDateTime::now_utc(),
        )
        .await
        .map_err(|error| match error {
            ConfirmationError::Invalid => {
                ApiError::new(StatusCode::BAD_REQUEST, "AUTH_ACTION_INVALID", id)
            }
            ConfirmationError::Expired => {
                ApiError::new(StatusCode::CONFLICT, "AUTH_ACTION_EXPIRED", id)
            }
            ConfirmationError::Consumed => {
                ApiError::new(StatusCode::CONFLICT, "AUTH_ACTION_CONSUMED", id)
            }
            ConfirmationError::Unavailable => ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "DEPENDENCY_UNAVAILABLE",
                id,
            ),
        })?;
    Ok(Json(json!({"status":"verified"})).into_response())
}
