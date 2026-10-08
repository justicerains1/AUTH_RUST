//! Current-user grants. Browser session authority is checked again by every repository action.
use crate::{
    accounts::AuthAppState,
    security::{ApiError, RequestId, TrustedSource},
};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get},
};
use identity_store::{
    repository::Digest,
    tokens::{TokenAudit, TokenError},
};
use uuid::Uuid;
pub fn grant_routes(state: AuthAppState) -> Router {
    Router::new()
        .route("/api/v1/me/grants", get(list))
        .route("/api/v1/me/grants/{id}", delete(revoke))
        .with_state(state)
}
fn current(state: &AuthAppState, request: &Request) -> Result<(Digest, Uuid), ApiError> {
    let id = request
        .extensions()
        .get::<RequestId>()
        .map_or_else(Uuid::new_v4, |id| id.0);
    let hash = state
        .security
        .identity_digest(request.headers())
        .map_err(|_| ApiError::new(StatusCode::UNAUTHORIZED, "AUTH_SESSION_REQUIRED", id))?
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "AUTH_SESSION_REQUIRED", id))?;
    Ok((hash, id))
}
fn error(failure: TokenError, id: Uuid) -> ApiError {
    match failure {
        TokenError::Unavailable | TokenError::SigningFailed => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "DEPENDENCY_UNAVAILABLE",
            id,
        ),
        TokenError::NotFound => ApiError::new(StatusCode::NOT_FOUND, "RESOURCE_NOT_FOUND", id),
        TokenError::InvalidGrant => ApiError::new(StatusCode::BAD_REQUEST, "INPUT_INVALID", id),
        _ => ApiError::new(StatusCode::UNAUTHORIZED, "AUTH_SESSION_REQUIRED", id),
    }
}
async fn list(State(state): State<AuthAppState>, request: Request) -> Result<Response, ApiError> {
    let (hash, id) = current(&state, &request)?;
    let mut limit = 20;
    let mut seen = false;
    let mut cursor = None;
    for (key, value) in request
        .uri()
        .query()
        .map(|query| url::form_urlencoded::parse(query.as_bytes()))
        .into_iter()
        .flatten()
    {
        match key.as_ref() {
            "limit" if !seen => {
                seen = true;
                limit = value
                    .parse::<u32>()
                    .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "INPUT_INVALID", id))?;
            }
            "cursor" if cursor.is_none() => cursor = Some(value.into_owned()),
            _ => return Err(ApiError::new(StatusCode::BAD_REQUEST, "INPUT_INVALID", id)),
        }
    }
    if !(1..=100).contains(&limit) {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "INPUT_INVALID", id));
    }
    Ok(Json(
        state
            .tokens
            .grant_page(hash, limit, cursor.as_deref(), &state.security.cursor_key())
            .await
            .map_err(|failure| error(failure, id))?,
    )
    .into_response())
}
async fn revoke(State(state): State<AuthAppState>, request: Request) -> Result<Response, ApiError> {
    let (hash, id) = current(&state, &request)?;
    let target = request
        .uri()
        .path()
        .rsplit('/')
        .next()
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "INPUT_INVALID", id))?;
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
    state
        .tokens
        .revoke_grant(
            hash,
            target,
            TokenAudit {
                request_id: id,
                source_hash: state.security.source_digest(source),
            },
        )
        .await
        .map_err(|failure| error(failure, id))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
