//! RP logout only revokes after a persisted browser-bound confirmation and explicit CSRF-protected POST.
use crate::{
    accounts::AuthAppState,
    oidc::{parse_form, protocol_error},
    security::{ApiError, RequestId, TrustedSource, read_json, unique_header},
};
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use identity_core::security::{Token, token_digest};
use identity_store::{
    repository::Digest,
    tokens::{LogoutConfirmInput, RpLogoutInput, TokenError},
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;
use zeroize::Zeroizing;

pub fn logout_routes(state: AuthAppState) -> Router {
    Router::new()
        .route("/oauth/logout", get(begin).post(begin))
        .route("/oauth/logout/confirm", post(confirm))
        .with_state(state)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmRequest {
    confirmation_id: Uuid,
    decision: Decision,
}
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Decision {
    Logout,
    Cancel,
}
fn error(error: TokenError) -> Response {
    match error {
        TokenError::Unavailable | TokenError::SigningFailed => {
            protocol_error(StatusCode::SERVICE_UNAVAILABLE, "temporarily_unavailable")
        }
        _ => protocol_error(StatusCode::BAD_REQUEST, "invalid_request"),
    }
}
fn redirect(location: &str) -> Response {
    match HeaderValue::from_str(location) {
        Ok(value) => {
            let mut response = StatusCode::FOUND.into_response();
            response.headers_mut().insert(header::LOCATION, value);
            response
        }
        Err(_) => protocol_error(StatusCode::BAD_REQUEST, "invalid_request"),
    }
}
async fn begin(State(state): State<AuthAppState>, request: Request) -> Response {
    let headers = request.headers().clone();
    let session_hash = match state.security.identity_digest(&headers) {
        Ok(hash) => hash,
        Err(_) => return protocol_error(StatusCode::BAD_REQUEST, "invalid_request"),
    };
    let mut preauth_hash = match state.security.preauth_digest(&headers) {
        Ok(hash) => hash,
        Err(_) => return protocol_error(StatusCode::BAD_REQUEST, "invalid_request"),
    };
    let form = if request.method() == Method::GET {
        request.uri().query().unwrap_or("").to_string()
    } else {
        if !unique_header(&headers, "content-type")
            .ok()
            .flatten()
            .is_some_and(|value| {
                value
                    .split(';')
                    .next()
                    .is_some_and(|kind| kind.trim() == "application/x-www-form-urlencoded")
            })
        {
            return protocol_error(StatusCode::BAD_REQUEST, "invalid_request");
        }
        let body = match to_bytes(request.into_body(), 65536).await {
            Ok(body) => body,
            Err(_) => return protocol_error(StatusCode::BAD_REQUEST, "invalid_request"),
        };
        match String::from_utf8(body.to_vec()) {
            Ok(form) => form,
            Err(_) => return protocol_error(StatusCode::BAD_REQUEST, "invalid_request"),
        }
    };
    let form = Zeroizing::new(form);
    let values = match parse_form(&form) {
        Ok(values) => values,
        Err(_) => return protocol_error(StatusCode::BAD_REQUEST, "invalid_request"),
    };
    if values.keys().any(|key| {
        !matches!(
            key.as_str(),
            "id_token_hint" | "post_logout_redirect_uri" | "state"
        )
    }) {
        return protocol_error(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let valid = match preauth_hash {
        Some(hash) => match state.oauth.valid_preauth(hash).await {
            Ok(valid) => valid,
            Err(_) => {
                return protocol_error(StatusCode::SERVICE_UNAVAILABLE, "temporarily_unavailable");
            }
        },
        None => false,
    };
    let (new_hash, cookie) = match state.security.ensure_preauth(&headers, valid).await {
        Ok(result) => result,
        Err(_) => {
            return protocol_error(StatusCode::SERVICE_UNAVAILABLE, "temporarily_unavailable");
        }
    };
    preauth_hash = Some(new_hash);
    let result = state
        .tokens
        .prepare_logout(
            &RpLogoutInput {
                session_hash,
                preauth_hash,
                id_token_hint: values
                    .get("id_token_hint")
                    .map(|value| Zeroizing::new(value.clone())),
                post_logout_redirect_uri: values.get("post_logout_redirect_uri").cloned(),
                state: values.get("state").cloned(),
            },
            &state.signer,
        )
        .await;
    let mut response = match result {
        Ok(prepared) => redirect(&format!(
            "/logout?confirmation={}",
            prepared.confirmation_id
        )),
        Err(failure) => error(failure),
    };
    if let Some(cookie) = cookie {
        response.headers_mut().append(header::SET_COOKIE, cookie);
    }
    response
}
async fn confirm(state: State<AuthAppState>, request: Request) -> Response {
    match confirm_json(state, request).await {
        Ok(response) => response,
        Err(failure) => protocol_error(
            if failure.status.is_server_error() {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                failure.status
            },
            if failure.status.is_server_error() {
                "temporarily_unavailable"
            } else {
                "invalid_request"
            },
        ),
    }
}
async fn confirm_json(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
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
    let session_hash = state
        .security
        .identity_digest(request.headers())
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "INPUT_INVALID", id))?;
    let preauth_hash = state
        .security
        .preauth_digest(request.headers())
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "INPUT_INVALID", id))?;
    let input = read_json::<ConfirmRequest>(request).await?;
    let token = Token::generate().map_err(|_| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "DEPENDENCY_UNAVAILABLE",
            id,
        )
    })?;
    let csrf = Token::generate().map_err(|_| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "DEPENDENCY_UNAVAILABLE",
            id,
        )
    })?;
    match state
        .tokens
        .confirm_logout(&LogoutConfirmInput {
            confirmation_id: input.confirmation_id,
            session_hash,
            preauth_hash,
            logout: matches!(input.decision, Decision::Logout),
            new_preauth_hash: Digest::from_bytes(token_digest(token.expose())),
            new_preauth_csrf_hash: Digest::from_bytes(token_digest(csrf.expose())),
            request_id: id,
            source_hash: state.security.source_digest(source),
        })
        .await
    {
        Err(failure) => Ok(error(failure)),
        Ok(confirmed) => {
            let mut response =
                Json(json!({"status":"completed","redirect_to":confirmed.redirect_to}))
                    .into_response();
            if confirmed.rotated {
                response.headers_mut().append(
                    header::SET_COOKIE,
                    state.security.clear_identity_cookie().map_err(|_| {
                        ApiError::new(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "DEPENDENCY_UNAVAILABLE",
                            id,
                        )
                    })?,
                );
                response.headers_mut().append(
                    header::SET_COOKIE,
                    state.security.preauth_cookie(token.expose()).map_err(|_| {
                        ApiError::new(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "DEPENDENCY_UNAVAILABLE",
                            id,
                        )
                    })?,
                );
            }
            Ok(response)
        }
    }
}
