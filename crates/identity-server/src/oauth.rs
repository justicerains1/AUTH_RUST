//! OAuth authorization entry points and browser-owned consent APIs; tokens are implemented separately.
use crate::{
    accounts::AuthAppState,
    security::{ApiError, LimitDecision, LimitPolicy, RequestId, TrustedSource, read_json},
};
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use identity_core::oauth::{AuthorizationRequest, OAuthInputError, valid_state};
use identity_store::oauth::{BrowserBinding, OAuthAudit, OAuthStoreError, PrepareOutcome};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

pub fn oauth_routes(state: AuthAppState) -> Router {
    Router::new()
        .route("/oauth/authorize", get(authorize).post(authorize))
        .route("/api/v1/oauth/transactions/{id}", get(transaction))
        .route("/api/v1/oauth/transactions/{id}/decision", post(decision))
        .with_state(state)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionRequest {
    decision: Decision,
}
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Decision {
    Approve,
    Deny,
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
pub(crate) fn local_error(status: StatusCode) -> Response {
    (status,Html("<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><title>授权请求无法完成</title></head><body><h1>授权请求无法完成</h1><p>请返回应用重新发起授权。</p></body></html>" )).into_response()
}
fn redirect(location: &str) -> Response {
    match HeaderValue::from_str(location) {
        Ok(header) => {
            let mut response = StatusCode::FOUND.into_response();
            response.headers_mut().insert(header::LOCATION, header);
            response
        }
        Err(_) => local_error(StatusCode::BAD_REQUEST),
    }
}
fn binding(state: &AuthAppState, request: &Request) -> Result<BrowserBinding, ApiError> {
    let (id, _) = context(request)?;
    Ok(BrowserBinding {
        session_hash: state
            .security
            .identity_digest(request.headers())
            .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "INPUT_INVALID", id))?,
        preauth_hash: state
            .security
            .preauth_digest(request.headers())
            .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "INPUT_INVALID", id))?,
    })
}
fn api_error(error: OAuthStoreError, id: Uuid) -> ApiError {
    let (status, code) = match error {
        OAuthStoreError::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "DEPENDENCY_UNAVAILABLE"),
        OAuthStoreError::Expired => (StatusCode::CONFLICT, "OAUTH_TRANSACTION_EXPIRED"),
        OAuthStoreError::Consumed => (StatusCode::CONFLICT, "OAUTH_TRANSACTION_CONSUMED"),
        OAuthStoreError::LoginRequired => (StatusCode::UNAUTHORIZED, "AUTH_SESSION_REQUIRED"),
        _ => (StatusCode::NOT_FOUND, "RESOURCE_NOT_FOUND"),
    };
    ApiError::new(status, code, id)
}

async fn authorize(State(state): State<AuthAppState>, request: Request) -> Response {
    let (id, source) = match context(&request) {
        Ok(context) => context,
        Err(_) => return local_error(StatusCode::BAD_REQUEST),
    };
    let headers = request.headers().clone();
    let mut browser = match binding(&state, &request) {
        Ok(binding) => binding,
        Err(_) => return local_error(StatusCode::BAD_REQUEST),
    };
    let form = if request.method() == Method::GET {
        request.uri().query().unwrap_or("").to_string()
    } else {
        if !crate::security::unique_header(request.headers(), "content-type")
            .ok()
            .flatten()
            .is_some_and(|value| {
                value
                    .split(';')
                    .next()
                    .is_some_and(|kind| kind.trim() == "application/x-www-form-urlencoded")
            })
        {
            return local_error(StatusCode::BAD_REQUEST);
        }
        let body = match to_bytes(request.into_body(), 65536).await {
            Ok(body) => body,
            Err(_) => return local_error(StatusCode::BAD_REQUEST),
        };
        match String::from_utf8(body.to_vec()) {
            Ok(form) => form,
            Err(_) => return local_error(StatusCode::BAD_REQUEST),
        }
    };
    let input = match AuthorizationRequest::parse(&form) {
        Ok(input) => input,
        Err(error) => return validated_input_error(&state, &form, error).await,
    };
    let limits = state
        .security
        .check_limit(LimitPolicy::Preauth, source, None, None)
        .await;
    match limits {
        Err(_) => return local_error(StatusCode::SERVICE_UNAVAILABLE),
        Ok(LimitDecision::Limited { retry_after, .. }) => {
            let mut response = local_error(StatusCode::TOO_MANY_REQUESTS);
            if let Ok(value) = HeaderValue::from_str(&retry_after.to_string()) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
            return response;
        }
        Ok(LimitDecision::Allowed) => {}
    }
    let existing_is_valid = match browser.preauth_hash {
        Some(hash) => match state.oauth.valid_preauth(hash).await {
            Ok(valid) => valid,
            Err(_) => return local_error(StatusCode::SERVICE_UNAVAILABLE),
        },
        None => false,
    };
    let (cookie, preauth) = match state
        .security
        .ensure_preauth(&headers, existing_is_valid)
        .await
    {
        Ok((hash, cookie)) => (cookie, hash),
        Err(_) => return local_error(StatusCode::SERVICE_UNAVAILABLE),
    };
    browser.preauth_hash = Some(preauth);
    let outcome = state
        .oauth
        .prepare(
            &input,
            browser,
            OAuthAudit {
                request_id: id,
                source_hash: state.security.source_digest(source),
            },
        )
        .await;
    let mut response = match outcome {
        Ok(PrepareOutcome::NeedLogin { transaction_id }) => {
            redirect(&format!("/login?transaction={transaction_id}"))
        }
        Ok(PrepareOutcome::Consent { transaction_id }) => {
            redirect(&format!("/oauth/consent/{transaction_id}"))
        }
        Ok(PrepareOutcome::ReadyCode(code)) => match code.redirect_to() {
            Ok(location) => redirect(&location),
            Err(_) => local_error(StatusCode::SERVICE_UNAVAILABLE),
        },
        Ok(PrepareOutcome::ProtocolError { redirect_to, .. }) => redirect(&redirect_to),
        Err(OAuthStoreError::Unavailable) => local_error(StatusCode::SERVICE_UNAVAILABLE),
        Err(_) => local_error(StatusCode::BAD_REQUEST),
    };
    if let Some(cookie) = cookie {
        response.headers_mut().append(header::SET_COOKIE, cookie);
    }
    response
}
async fn validated_input_error(
    state: &AuthAppState,
    form: &str,
    error: OAuthInputError,
) -> Response {
    let mut values = BTreeMap::new();
    for (name, value) in url::form_urlencoded::parse(form.as_bytes()) {
        if values
            .insert(name.into_owned(), value.into_owned())
            .is_some()
        {
            return local_error(StatusCode::BAD_REQUEST);
        }
    }
    let (Some(client), Some(callback), Some(parameter)) = (
        values.get("client_id"),
        values.get("redirect_uri"),
        values.get("state"),
    ) else {
        return local_error(StatusCode::BAD_REQUEST);
    };
    if !valid_state(parameter) {
        return local_error(StatusCode::BAD_REQUEST);
    }
    match state.oauth.validate_callback(client, callback).await {
        Ok(true) => {}
        Ok(false) => return local_error(StatusCode::BAD_REQUEST),
        Err(_) => return local_error(StatusCode::SERVICE_UNAVAILABLE),
    }
    let mut callback = match url::Url::parse(callback) {
        Ok(callback) => callback,
        Err(_) => return local_error(StatusCode::BAD_REQUEST),
    };
    callback
        .query_pairs_mut()
        .append_pair("error", &error.to_string())
        .append_pair("state", parameter);
    redirect(callback.as_str())
}
async fn transaction(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let (id, _) = context(&request)?;
    let target = request
        .uri()
        .path()
        .rsplit('/')
        .next()
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "INPUT_INVALID", id))?;
    let browser = binding(&state, &request)?;
    let view = state
        .oauth
        .view(target, browser)
        .await
        .map_err(|error| api_error(error, id))?;
    Ok(Json(view).into_response())
}
async fn decision(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let (id, source) = context(&request)?;
    let browser = binding(&state, &request)?;
    let target = request
        .uri()
        .path()
        .trim_end_matches("/decision")
        .rsplit('/')
        .next()
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "INPUT_INVALID", id))?;
    let input = read_json::<DecisionRequest>(request).await?;
    let audit = OAuthAudit {
        request_id: id,
        source_hash: state.security.source_digest(source),
    };
    let location = match input.decision {
        Decision::Approve => state
            .oauth
            .decision(target, browser, true, audit)
            .await
            .and_then(|code| code.redirect_to()),
        Decision::Deny => state.oauth.deny(target, browser, audit).await,
    }
    .map_err(|error| api_error(error, id))?;
    Ok(Json(json!({"status":"completed","redirect_to":location})).into_response())
}
