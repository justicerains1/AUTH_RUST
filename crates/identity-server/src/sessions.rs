//! Password login and authority-backed user/session endpoints. TOTP verification remains a later task.
use crate::{
    accounts::AuthAppState,
    security::{ApiError, LimitDecision, LimitPolicy, RequestId, TrustedSource, read_json},
};
use axum::{
    Json, Router,
    extract::Request,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use identity_core::security::{Password, SecurityError, Token, normalize_email, token_digest};
use identity_store::{
    repository::Digest,
    sessions::{LoginCommitInput, LoginOutcome, LogoutInput, SessionError, SessionRevocationInput},
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginRequest {
    email: String,
    password: String,
}
impl Drop for LoginRequest {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

pub fn session_routes(state: AuthAppState) -> Router {
    Router::new()
        .route("/api/v1/auth/login/password", post(password_login))
        .route("/api/v1/me", get(me))
        .route("/api/v1/me/sessions", get(list_sessions))
        .route("/api/v1/me/sessions/{id}", delete(revoke_session))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/auth/logout-all", post(logout_all))
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
fn credentials(id: Uuid) -> ApiError {
    ApiError::new(StatusCode::UNAUTHORIZED, "AUTH_INVALID_CREDENTIALS", id)
}
fn session_required(id: Uuid) -> ApiError {
    ApiError::new(StatusCode::UNAUTHORIZED, "AUTH_SESSION_REQUIRED", id)
}
fn digest(token: &Token) -> Digest {
    Digest::from_bytes(token_digest(token.expose()))
}
fn store_error(error: SessionError, id: Uuid) -> ApiError {
    match error {
        SessionError::Unavailable => unavailable(id),
        SessionError::NotFound => ApiError::new(StatusCode::NOT_FOUND, "RESOURCE_NOT_FOUND", id),
        SessionError::InvalidCursor => ApiError::new(StatusCode::BAD_REQUEST, "INPUT_INVALID", id),
        _ => session_required(id),
    }
}
fn password_error(error: SecurityError, id: Uuid) -> ApiError {
    let mut result = unavailable(id);
    if matches!(error, SecurityError::Busy) {
        result.retry_after = Some(1);
    }
    result
}

async fn password_login(
    axum::extract::State(state): axum::extract::State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let (id, source) = context(&request)?;
    let preauth_hash = state
        .security
        .preauth_digest(request.headers())
        .map_err(|_| credentials(id))?;
    let old_session_hash = state
        .security
        .identity_digest(request.headers())
        .map_err(|_| credentials(id))?;
    if preauth_hash.is_none() && old_session_hash.is_none() {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "AUTH_CSRF_INVALID",
            id,
        ));
    }
    let user_agent = redacted_agent(
        request
            .headers()
            .get(header::USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .unwrap_or(""),
    );
    let mut value = read_json::<LoginRequest>(request).await?;
    let password = Zeroizing::new(std::mem::take(&mut value.password));
    let email = normalize_email(&value.email)
        .map_err(|_| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "INPUT_INVALID", id))?;
    if password.len() > 512 || password.chars().count() > 128 {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "INPUT_INVALID",
            id,
        ));
    }
    match state
        .security
        .check_limit(LimitPolicy::PasswordLogin, source, Some(&email), None)
        .await
        .map_err(|_| unavailable(id))?
    {
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
    let candidate = state
        .sessions
        .credential_by_email(&email)
        .await
        .map_err(|error| store_error(error, id))?;
    let Some(candidate) = candidate else {
        state
            .inner
            .passwords
            .verify_unknown(&password)
            .await
            .map_err(|error| password_error(error, id))?;
        return Err(credentials(id));
    };
    let verification = state
        .inner
        .passwords
        .verify(&password, &candidate.password_hash)
        .await
        .map_err(|error| password_error(error, id))?;
    if !verification.valid || !candidate.verified || candidate.status != "active" {
        return Err(credentials(id));
    }
    let upgraded = if verification.needs_upgrade {
        let password = Password::new(&password).map_err(|_| credentials(id))?;
        Some(
            state
                .inner
                .passwords
                .hash(&password)
                .await
                .map_err(|error| password_error(error, id))?,
        )
    } else {
        None
    };
    let session_token = Token::generate().map_err(|_| unavailable(id))?;
    let session_csrf = Token::generate().map_err(|_| unavailable(id))?;
    let preauth_token = Token::generate().map_err(|_| unavailable(id))?;
    let preauth_csrf = Token::generate().map_err(|_| unavailable(id))?;
    let outcome = state
        .sessions
        .complete_password_login(&LoginCommitInput {
            user_id: candidate.id,
            expected_credential_version: candidate.credential_version,
            preauth_hash,
            old_session_hash,
            session_id: Uuid::new_v4(),
            session_token_hash: digest(&session_token),
            session_csrf_hash: digest(&session_csrf),
            new_preauth_hash: digest(&preauth_token),
            new_preauth_csrf_hash: digest(&preauth_csrf),
            user_agent,
            upgraded_password_hash: upgraded,
            request_id: id,
            source_hash: state.security.source_digest(source),
        })
        .await
        .map_err(|error| {
            if matches!(error, SessionError::Unavailable) {
                unavailable(id)
            } else {
                credentials(id)
            }
        })?;
    match outcome {
        LoginOutcome::Authenticated { user, session } => {
            let mut response=Json(json!({"status":"authenticated","user":user,"session":session,"csrf_token":session_csrf.expose()})).into_response();
            response.headers_mut().append(
                header::SET_COOKIE,
                state
                    .security
                    .identity_cookie(session_token.expose())
                    .map_err(|_| unavailable(id))?,
            );
            response.headers_mut().append(
                header::SET_COOKIE,
                state
                    .security
                    .preauth_cookie(preauth_token.expose())
                    .map_err(|_| unavailable(id))?,
            );
            Ok(response)
        }
        LoginOutcome::MfaRequired {
            challenge_id,
            expires_at,
            methods,
        } => {
            let mut response=Json(json!({"status":"mfa_required","challenge_id":challenge_id,"purpose":"login","methods":methods,"expires_at":expires_at.format(&time::format_description::well_known::Rfc3339).map_err(|_|unavailable(id))?})).into_response();
            response.headers_mut().append(
                header::SET_COOKIE,
                state
                    .security
                    .preauth_cookie(preauth_token.expose())
                    .map_err(|_| unavailable(id))?,
            );
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
fn redacted_agent(raw: &str) -> String {
    if raw.contains("Edg/") {
        "Microsoft Edge"
    } else if raw.contains("Firefox/") {
        "Mozilla Firefox"
    } else if raw.contains("Chrome/") {
        "Google Chrome"
    } else if raw.contains("Safari/") {
        "Safari"
    } else {
        "未识别浏览器"
    }
    .to_string()
}

async fn me(
    axum::extract::State(state): axum::extract::State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let (id, _) = context(&request)?;
    let hash = state
        .security
        .identity_digest(request.headers())
        .map_err(|_| session_required(id))?
        .ok_or_else(|| session_required(id))?;
    let view = state
        .sessions
        .me(hash)
        .await
        .map_err(|error| store_error(error, id))?
        .ok_or_else(|| session_required(id))?;
    Ok(Json(view).into_response())
}
async fn list_sessions(
    axum::extract::State(state): axum::extract::State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let (id, _) = context(&request)?;
    let hash = state
        .security
        .identity_digest(request.headers())
        .map_err(|_| session_required(id))?
        .ok_or_else(|| session_required(id))?;
    let mut limit = 20;
    let mut cursor = None;
    let mut limit_seen = false;
    for (pair, value) in request
        .uri()
        .query()
        .map(|query| url::form_urlencoded::parse(query.as_bytes()))
        .into_iter()
        .flatten()
    {
        match pair.as_ref() {
            "limit" if !limit_seen => {
                limit_seen = true;
                limit = value
                    .parse::<u32>()
                    .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "INPUT_INVALID", id))?;
            }
            "cursor" if cursor.is_none() => {
                cursor = Some(value.into_owned());
            }
            _ => return Err(ApiError::new(StatusCode::BAD_REQUEST, "INPUT_INVALID", id)),
        }
    }
    if !(1..=100).contains(&limit) {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "INPUT_INVALID", id));
    }
    let view = state
        .sessions
        .session_page(hash, limit, cursor.as_deref(), &state.security.cursor_key())
        .await
        .map_err(|error| store_error(error, id))?;
    Ok(Json(view).into_response())
}
async fn revoke_session(
    axum::extract::State(state): axum::extract::State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let (id, source) = context(&request)?;
    let current_hash = state
        .security
        .identity_digest(request.headers())
        .map_err(|_| session_required(id))?
        .ok_or_else(|| session_required(id))?;
    let target_id = request
        .uri()
        .path()
        .rsplit('/')
        .next()
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "INPUT_INVALID", id))?;
    let current = state
        .sessions
        .revoke_session(&SessionRevocationInput {
            current_hash,
            target_id,
            request_id: id,
            source_hash: state.security.source_digest(source),
        })
        .await
        .map_err(|error| store_error(error, id))?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    if current {
        response.headers_mut().append(
            header::SET_COOKIE,
            state
                .security
                .clear_identity_cookie()
                .map_err(|_| unavailable(id))?,
        );
    }
    Ok(response)
}
async fn logout(
    state: axum::extract::State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    finish_logout(state.0, request, false).await
}
async fn logout_all(
    state: axum::extract::State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    finish_logout(state.0, request, true).await
}
async fn finish_logout(
    state: AuthAppState,
    request: Request,
    all: bool,
) -> Result<Response, ApiError> {
    let (id, source) = context(&request)?;
    let current_hash = state
        .security
        .identity_digest(request.headers())
        .map_err(|_| session_required(id))?;
    let token = Token::generate().map_err(|_| unavailable(id))?;
    let csrf = Token::generate().map_err(|_| unavailable(id))?;
    let changed = state
        .sessions
        .logout(&LogoutInput {
            current_hash,
            all,
            new_preauth_hash: digest(&token),
            new_preauth_csrf_hash: digest(&csrf),
            request_id: id,
            source_hash: state.security.source_digest(source),
        })
        .await
        .map_err(|error| store_error(error, id))?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        state
            .security
            .clear_identity_cookie()
            .map_err(|_| unavailable(id))?,
    );
    if changed {
        response.headers_mut().append(
            header::SET_COOKIE,
            state
                .security
                .preauth_cookie(token.expose())
                .map_err(|_| unavailable(id))?,
        );
    }
    Ok(response)
}
