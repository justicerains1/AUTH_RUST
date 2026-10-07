//! Password recovery and mutation endpoints; all security state mutations are delegated to one DB transaction.
use crate::{
    accounts::{AuthAppState, accepted, mail_budget},
    security::{
        ApiError, LimitDecision, LimitPolicy, ReauthFailure, ReauthMethod, RequestId,
        RequiredStrength, TrustedSource, read_json,
    },
};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use identity_core::security::{Password, SecurityError, normalize_email, token_digest};
use identity_store::{
    passwords::{
        ChangeCommitInput, PasswordError, ReauthCommitInput, ReauthOutcome, ResetCommitInput,
    },
    repository::Digest,
};
use serde::Deserialize;
use serde_json::json;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmailRequest {
    email: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResetRequest {
    token: String,
    password: String,
}
impl Drop for ResetRequest {
    fn drop(&mut self) {
        self.token.zeroize();
        self.password.zeroize();
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewPasswordRequest {
    new_password: String,
}
impl Drop for NewPasswordRequest {
    fn drop(&mut self) {
        self.new_password.zeroize();
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasswordRequest {
    password: String,
}
impl Drop for PasswordRequest {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

pub fn password_routes(state: AuthAppState) -> Router {
    Router::new()
        .route("/api/v1/auth/password-reset/request", post(request_reset))
        .route("/api/v1/auth/password-reset/confirm", post(confirm_reset))
        .route("/api/v1/me/password/change", post(change_password))
        .route("/api/v1/me/reauth/password", post(reauthenticate))
        .with_state(state)
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
        .ok_or_else(|| unavailable(id))?;
    Ok((id, source))
}
fn unavailable(id: Uuid) -> ApiError {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "DEPENDENCY_UNAVAILABLE",
        id,
    )
}
fn password_error(error: SecurityError, id: Uuid) -> ApiError {
    let mut result = unavailable(id);
    if matches!(error, SecurityError::Busy) {
        result.retry_after = Some(1);
    }
    result
}
fn error_response(error: PasswordError, id: Uuid) -> Response {
    if let PasswordError::ReauthRequired { strong, methods } = error {
        return ReauthFailure {
            request_id: id,
            required_strength: if strong {
                RequiredStrength::Strong
            } else {
                RequiredStrength::Password
            },
            methods: methods
                .iter()
                .filter_map(|method| match method.as_str() {
                    "password" => Some(ReauthMethod::Password),
                    "totp" => Some(ReauthMethod::Totp),
                    "recovery_code" => Some(ReauthMethod::RecoveryCode),
                    "passkey" => Some(ReauthMethod::Passkey),
                    _ => None,
                })
                .collect(),
        }
        .into_response();
    }
    let (status, code) = match error {
        PasswordError::InvalidSession => (StatusCode::UNAUTHORIZED, "AUTH_SESSION_REQUIRED"),
        PasswordError::InvalidCredentials => (StatusCode::UNAUTHORIZED, "AUTH_INVALID_CREDENTIALS"),
        PasswordError::InvalidAction => (StatusCode::BAD_REQUEST, "AUTH_ACTION_INVALID"),
        PasswordError::ExpiredAction => (StatusCode::CONFLICT, "AUTH_ACTION_EXPIRED"),
        PasswordError::ConsumedAction => (StatusCode::CONFLICT, "AUTH_ACTION_CONSUMED"),
        _ => (StatusCode::SERVICE_UNAVAILABLE, "DEPENDENCY_UNAVAILABLE"),
    };
    ApiError::new(status, code, id).into_response()
}
async fn budget(
    state: &AuthAppState,
    policy: LimitPolicy,
    source: std::net::IpAddr,
    email: Option<&str>,
    id: Uuid,
) -> Result<(), ApiError> {
    match state
        .security
        .check_limit(policy, source, email, None)
        .await
        .map_err(|_| unavailable(id))?
    {
        LimitDecision::Allowed => Ok(()),
        LimitDecision::Limited { retry_after, .. } => Err(ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "RATE_LIMITED",
            request_id: id,
            retry_after: Some(retry_after),
        }),
    }
}
fn session(state: &AuthAppState, request: &Request, id: Uuid) -> Result<Digest, ApiError> {
    state
        .security
        .identity_digest(request.headers())
        .map_err(|_| ApiError::new(StatusCode::UNAUTHORIZED, "AUTH_SESSION_REQUIRED", id))?
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "AUTH_SESSION_REQUIRED", id))
}

async fn request_reset(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let (id, source) = context(&request)?;
    let value = read_json::<EmailRequest>(request).await?;
    let email = normalize_email(&value.email)
        .map_err(|_| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "INPUT_INVALID", id))?;
    if !mail_budget(&state, source, &email, id).await? {
        return Ok(accepted());
    }
    match state
        .password_store
        .request_reset(
            &email,
            &state.inner.issuer,
            &state.inner.keys,
            state.security.source_digest(source),
            id,
        )
        .await
    {
        Ok(()) => Ok(accepted()),
        Err(error) => Ok(error_response(error, id)),
    }
}
async fn confirm_reset(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let (id, source) = context(&request)?;
    let mut value = read_json::<ResetRequest>(request).await?;
    let token = Zeroizing::new(std::mem::take(&mut value.token));
    let raw = Zeroizing::new(std::mem::take(&mut value.password));
    if token.len() != 43
        || BASE64_URL_SAFE_NO_PAD
            .decode(token.as_bytes())
            .map_or(true, |value| value.len() != 32)
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "AUTH_ACTION_INVALID",
            id,
        ));
    }
    let password = Password::new(&raw)
        .map_err(|_| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "INPUT_INVALID", id))?;
    budget(&state, LimitPolicy::Challenge, source, None, id).await?;
    let hash = state
        .inner
        .passwords
        .hash(&password)
        .await
        .map_err(|error| password_error(error, id))?;
    match state
        .password_store
        .confirm_reset(
            &ResetCommitInput {
                token_hash: Digest::from_bytes(token_digest(&token)),
                new_password_hash: hash,
                source_hash: state.security.source_digest(source),
                request_id: id,
            },
            &state.inner.keys,
        )
        .await
    {
        Err(error) => Ok(error_response(error, id)),
        Ok(()) => {
            let mut response =
                Json(json!({"status":"password_reset","next":"login"})).into_response();
            response.headers_mut().append(
                header::SET_COOKIE,
                state
                    .security
                    .clear_identity_cookie()
                    .map_err(|_| unavailable(id))?,
            );
            Ok(response)
        }
    }
}
async fn reauthenticate(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let (id, source) = context(&request)?;
    let hash = session(&state, &request, id)?;
    let mut value = read_json::<PasswordRequest>(request).await?;
    let password = Zeroizing::new(std::mem::take(&mut value.password));
    if password.len() > 512 || password.chars().count() > 128 {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "INPUT_INVALID",
            id,
        ));
    }
    let candidate = match state.password_store.credential_by_session(hash).await {
        Ok(Some(candidate)) => candidate,
        Ok(None) => {
            return Err(ApiError::new(
                StatusCode::UNAUTHORIZED,
                "AUTH_SESSION_REQUIRED",
                id,
            ));
        }
        Err(error) => return Ok(error_response(error, id)),
    };
    budget(
        &state,
        LimitPolicy::PasswordLogin,
        source,
        Some(&candidate.email),
        id,
    )
    .await?;
    if !state
        .inner
        .passwords
        .verify(&password, &candidate.password_hash)
        .await
        .map_err(|error| password_error(error, id))?
        .valid
    {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "AUTH_INVALID_CREDENTIALS",
            id,
        ));
    }
    match state.password_store.confirm_password_reauthentication(&ReauthCommitInput{session_hash:hash,expected_credential_version:candidate.credential_version,source_hash:state.security.source_digest(source),request_id:id}).await {
        Ok(ReauthOutcome::Confirmed{reauthenticated_at,valid_until,amr})=>Ok(Json(json!({"status":"reauthenticated","reauthenticated_at":reauthenticated_at.format(&Rfc3339).map_err(|_|unavailable(id))?,"valid_until":valid_until.format(&Rfc3339).map_err(|_|unavailable(id))?,"strong":false,"amr":amr})).into_response()),
        Ok(ReauthOutcome::MfaRequired{challenge_id,expires_at,methods})=>Ok(Json(json!({"status":"mfa_required","challenge_id":challenge_id,"purpose":"reauthentication","methods":methods,"expires_at":expires_at.format(&Rfc3339).map_err(|_|unavailable(id))?})).into_response()),Err(error)=>Ok(error_response(error,id))
    }
}
async fn change_password(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let (id, source) = context(&request)?;
    let hash = session(&state, &request, id)?;
    let mut value = read_json::<NewPasswordRequest>(request).await?;
    let raw = Zeroizing::new(std::mem::take(&mut value.new_password));
    let password = Password::new(&raw)
        .map_err(|_| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "INPUT_INVALID", id))?;
    budget(&state, LimitPolicy::Challenge, source, None, id).await?;
    let candidate = match state.password_store.credential_by_session(hash).await {
        Ok(Some(candidate)) => candidate,
        Ok(None) => {
            return Err(ApiError::new(
                StatusCode::UNAUTHORIZED,
                "AUTH_SESSION_REQUIRED",
                id,
            ));
        }
        Err(error) => return Ok(error_response(error, id)),
    };
    let new_hash = state
        .inner
        .passwords
        .hash(&password)
        .await
        .map_err(|error| password_error(error, id))?;
    match state
        .password_store
        .change_password(
            &ChangeCommitInput {
                session_hash: hash,
                expected_credential_version: candidate.credential_version,
                new_password_hash: new_hash,
                source_hash: state.security.source_digest(source),
                request_id: id,
            },
            &state.inner.keys,
        )
        .await
    {
        Err(error) => Ok(error_response(error, id)),
        Ok(()) => {
            let mut response = StatusCode::NO_CONTENT.into_response();
            response.headers_mut().append(
                header::SET_COOKIE,
                state
                    .security
                    .clear_identity_cookie()
                    .map_err(|_| unavailable(id))?,
            );
            Ok(response)
        }
    }
}
