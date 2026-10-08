//! BFF browser surface: only application session cookies leave the server, never OAuth tokens.
use crate::{
    config::BffConfig,
    oidc::{Oidc, OidcError},
    store::{BffStore, LoginFlowInput, NewBffSession, StoreError},
};
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use identity_core::{
    clock::Clock,
    security::{AeadKeyRing, Token, constant_time_equal, keyed_account_digest, token_digest},
};
use serde_json::json;
use sqlx::PgPool;
use std::{collections::BTreeMap, sync::Arc};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct BffState {
    inner: Arc<BffInner>,
}
struct BffInner {
    store: BffStore,
    oidc: Oidc,
    origin: String,
    cookie: String,
    flow_cookie: String,
    secure: bool,
    csrf_key: Zeroizing<[u8; 32]>,
}
impl BffState {
    pub async fn new(
        config: &BffConfig,
        pool: PgPool,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, &'static str> {
        let keys = Arc::new(
            AeadKeyRing::load_file(&config.encryption_keys_file, &config.active_encryption_kid)
                .map_err(|_| "invalid BFF encryption configuration")?,
        );
        let csrf_key = keys
            .derive_hmac_key(&format!("bff:{}:csrf", config.namespace))
            .map_err(|_| "invalid BFF CSRF configuration")?;
        let store = BffStore::new(pool, config.namespace.clone(), keys, clock)
            .map_err(|_| "invalid BFF storage configuration")?;
        let oidc = Oidc::discover(config)
            .await
            .map_err(|_| "identity provider discovery unavailable")?;
        Ok(Self {
            inner: Arc::new(BffInner {
                store,
                oidc,
                origin: config.public_origin.origin().ascii_serialization(),
                cookie: config.cookie_name.clone(),
                flow_cookie: config.flow_cookie_name.clone(),
                secure: config.public_origin.scheme() == "https",
                csrf_key,
            }),
        })
    }
    fn cookie(&self, name: &str, value: &str, age: u32) -> Result<HeaderValue, BffError> {
        HeaderValue::from_str(&format!(
            "{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={age}{}",
            if self.inner.secure { "; Secure" } else { "" }
        ))
        .map_err(|_| BffError::Unavailable)
    }
    fn csrf(&self, cookie: &str) -> String {
        BASE64_URL_SAFE_NO_PAD.encode(keyed_account_digest(
            &self.inner.csrf_key,
            "browser-session",
            cookie,
        ))
    }
    fn invalid_session(&self) -> Result<Response, BffError> {
        self.clear_session(BffError::InvalidSession)
    }
    fn clear_session(&self, error: BffError) -> Result<Response, BffError> {
        let mut response = error.into_response();
        response
            .headers_mut()
            .append(header::SET_COOKIE, self.cookie(&self.inner.cookie, "", 0)?);
        Ok(response)
    }
}
#[derive(Debug)]
enum BffError {
    InvalidInput,
    InvalidSession,
    Forbidden,
    Unavailable,
}
impl IntoResponse for BffError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::InvalidInput => (StatusCode::BAD_REQUEST, "BFF_INVALID_INPUT", "请求输入无效"),
            Self::InvalidSession => (
                StatusCode::UNAUTHORIZED,
                "BFF_SESSION_REQUIRED",
                "请先登录此应用",
            ),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                "BFF_CSRF_INVALID",
                "安全验证失效，请刷新后重试",
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "BFF_UNAVAILABLE",
                "身份状态暂不可用，请稍后重试",
            ),
        };
        let mut response = (
            status,
            Json(json!({"error":{"code":code,"message":message,"request_id":Uuid::new_v4()}})),
        )
            .into_response();
        safe_headers(&mut response);
        response
    }
}
fn safe_headers(response: &mut Response) {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
}
pub fn router(state: BffState) -> Router {
    Router::new()
        .route(
            "/health/live",
            get(|| async { Json(json!({"status":"live"})) }),
        )
        .route("/bff/login", get(login))
        .route("/bff/callback", get(callback))
        .route("/bff/session", get(session))
        .route("/bff/csrf", get(csrf))
        .route("/bff/logout", post(local_logout))
        .route("/bff/identity-logout", post(identity_logout))
        .with_state(state)
}
fn unique<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>, BffError> {
    let mut values = headers.get_all(name).iter();
    let value = values.next();
    if values.next().is_some() {
        return Err(BffError::InvalidInput);
    }
    value
        .map(|value| value.to_str().map_err(|_| BffError::InvalidInput))
        .transpose()
}
fn cookie(headers: &HeaderMap, name: &str) -> Result<Option<String>, BffError> {
    let mut found = None;
    for header in headers.get_all(header::COOKIE) {
        let header = header.to_str().map_err(|_| BffError::InvalidInput)?;
        for piece in header.split(';') {
            if let Some((key, value)) = piece.trim().split_once('=')
                && key == name
            {
                if found.is_some()
                    || value.len() != 43
                    || BASE64_URL_SAFE_NO_PAD.decode(value).is_err()
                {
                    return Err(BffError::InvalidInput);
                }
                found = Some(value.to_string());
            }
        }
    }
    Ok(found)
}
fn redirect(location: &str) -> Result<Response, BffError> {
    let mut response = StatusCode::FOUND.into_response();
    response.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(location).map_err(|_| BffError::InvalidInput)?,
    );
    safe_headers(&mut response);
    Ok(response)
}
async fn login(State(state): State<BffState>) -> Result<Response, BffError> {
    let (url, csrf, nonce, verifier) = state
        .inner
        .oidc
        .authorization()
        .map_err(|_| BffError::Unavailable)?;
    let flow = Token::generate().map_err(|_| BffError::Unavailable)?;
    state
        .inner
        .store
        .begin_login(&LoginFlowInput {
            flow_cookie_hash: token_digest(flow.expose()),
            state_hash: token_digest(&csrf),
            nonce,
            pkce_verifier: verifier,
        })
        .await
        .map_err(|_| BffError::Unavailable)?;
    let mut response = redirect(url.as_str())?;
    response.headers_mut().append(
        header::SET_COOKIE,
        state.cookie(&state.inner.flow_cookie, flow.expose(), 300)?,
    );
    Ok(response)
}
async fn callback(State(state): State<BffState>, request: Request) -> Result<Response, BffError> {
    let mut values = BTreeMap::<String, Zeroizing<String>>::new();
    for (key, value) in request
        .uri()
        .query()
        .map(|query| url::form_urlencoded::parse(query.as_bytes()))
        .into_iter()
        .flatten()
    {
        if !matches!(
            key.as_ref(),
            "code" | "state" | "error" | "error_description" | "error_uri"
        ) || values
            .insert(key.into_owned(), Zeroizing::new(value.into_owned()))
            .is_some()
        {
            return Err(BffError::InvalidInput);
        }
    }
    let flow =
        cookie(request.headers(), &state.inner.flow_cookie)?.ok_or(BffError::InvalidInput)?;
    let returned_state = values.get("state").ok_or(BffError::InvalidInput)?;
    let secrets = state
        .inner
        .store
        .consume_login(token_digest(&flow), token_digest(returned_state))
        .await
        .map_err(|error| {
            if matches!(error, StoreError::Unavailable) {
                BffError::Unavailable
            } else {
                BffError::InvalidInput
            }
        })?;
    if values.contains_key("error") {
        return Err(BffError::InvalidInput);
    }
    let code = values.get("code").ok_or(BffError::InvalidInput)?;
    let (user, tokens) = state
        .inner
        .oidc
        .exchange(code, &secrets.pkce_verifier, &secrets.nonce)
        .await
        .map_err(|_| BffError::Unavailable)?;
    let token = Token::generate().map_err(|_| BffError::Unavailable)?;
    let old = cookie(request.headers(), &state.inner.cookie)?.map(|cookie| token_digest(&cookie));
    state
        .inner
        .store
        .create_session(&NewBffSession {
            cookie_hash: token_digest(token.expose()),
            user_id: user,
            tokens,
            old_cookie_hash: old,
        })
        .await
        .map_err(|_| BffError::Unavailable)?;
    let mut response = redirect("/")?;
    response.headers_mut().append(
        header::SET_COOKIE,
        state.cookie(&state.inner.cookie, token.expose(), 43200)?,
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        state.cookie(&state.inner.flow_cookie, "", 0)?,
    );
    Ok(response)
}
async fn session(State(state): State<BffState>, request: Request) -> Result<Response, BffError> {
    let cookie = cookie(request.headers(), &state.inner.cookie)?.ok_or(BffError::InvalidSession)?;
    let hash = token_digest(&cookie);
    let locked = match state.inner.store.lock_session(hash).await {
        Ok(locked) => locked,
        Err(StoreError::Unavailable) => return Err(BffError::Unavailable),
        Err(_) => {
            state
                .inner
                .store
                .delete_session(hash)
                .await
                .map_err(|_| BffError::Unavailable)?;
            return state.invalid_session();
        }
    };
    let user = locked.user_id;
    if locked.needs_refresh() {
        let current = locked.tokens();
        let snapshot = crate::store::BffTokens {
            access_token: current.access_token.clone(),
            refresh_token: current.refresh_token.clone(),
            id_token: current.id_token.clone(),
            access_expires_at: current.access_expires_at,
            session_expires_at: current.session_expires_at,
        };
        let refreshed = match state.inner.oidc.refresh(&snapshot).await {
            Ok((subject, tokens)) if subject == user => tokens,
            Ok(_) => {
                locked
                    .invalidate()
                    .await
                    .map_err(|_| BffError::Unavailable)?;
                return state.invalid_session();
            }
            Err(error) => {
                locked
                    .invalidate()
                    .await
                    .map_err(|_| BffError::Unavailable)?;
                return state.clear_session(if matches!(error, OidcError::InvalidGrant) {
                    BffError::InvalidSession
                } else {
                    BffError::Unavailable
                });
            }
        };
        locked
            .replace_tokens(&refreshed)
            .await
            .map_err(|_| BffError::Unavailable)?;
        let locked = state
            .inner
            .store
            .lock_session(hash)
            .await
            .map_err(|_| BffError::Unavailable)?;
        let access = Zeroizing::new(locked.tokens().access_token.clone());
        let active = state.inner.oidc.active(&access, user).await;
        match active {
            Ok(true) => locked.release().await.map_err(|_| BffError::Unavailable)?,
            Ok(false) => {
                locked
                    .invalidate()
                    .await
                    .map_err(|_| BffError::Unavailable)?;
                return state.invalid_session();
            }
            Err(_) => {
                locked.release().await.map_err(|_| BffError::Unavailable)?;
                return Err(BffError::Unavailable);
            }
        }
    } else {
        let access = Zeroizing::new(locked.tokens().access_token.clone());
        match state.inner.oidc.active(&access, user).await {
            Ok(true) => locked.release().await.map_err(|_| BffError::Unavailable)?,
            Ok(false) => {
                locked
                    .invalidate()
                    .await
                    .map_err(|_| BffError::Unavailable)?;
                return state.invalid_session();
            }
            Err(_) => {
                locked.release().await.map_err(|_| BffError::Unavailable)?;
                return Err(BffError::Unavailable);
            }
        }
    }
    let mut response = Json(json!({"user":{"sub":user},"authenticated":true})).into_response();
    safe_headers(&mut response);
    Ok(response)
}
async fn csrf(State(state): State<BffState>, request: Request) -> Result<Response, BffError> {
    let cookie = cookie(request.headers(), &state.inner.cookie)?.ok_or(BffError::InvalidSession)?;
    let mut response = Json(json!({"csrf_token":state.csrf(&cookie)})).into_response();
    safe_headers(&mut response);
    Ok(response)
}
fn mutation(state: &BffState, request: &Request) -> Result<String, BffError> {
    if unique(request.headers(), "origin")? != Some(&state.inner.origin) {
        return Err(BffError::Forbidden);
    }
    let cookie = cookie(request.headers(), &state.inner.cookie)?.ok_or(BffError::InvalidSession)?;
    let expected = state.csrf(&cookie);
    let token = unique(request.headers(), "x-csrf-token")?.ok_or(BffError::Forbidden)?;
    if !constant_time_equal(expected.as_bytes(), token.as_bytes()) {
        return Err(BffError::Forbidden);
    }
    Ok(cookie)
}
async fn local_logout(
    State(state): State<BffState>,
    request: Request,
) -> Result<Response, BffError> {
    finish_logout(state, request, false).await
}
async fn identity_logout(
    State(state): State<BffState>,
    request: Request,
) -> Result<Response, BffError> {
    finish_logout(state, request, true).await
}
async fn finish_logout(
    state: BffState,
    request: Request,
    identity: bool,
) -> Result<Response, BffError> {
    let cookie = mutation(&state, &request)?;
    let bytes = to_bytes(request.into_body(), 1024)
        .await
        .map_err(|_| BffError::InvalidInput)?;
    if !bytes.is_empty() {
        return Err(BffError::InvalidInput);
    }
    let token = match state.inner.store.lock_session(token_digest(&cookie)).await {
        Ok(locked) => {
            let token = Zeroizing::new(locked.tokens().refresh_token.clone());
            locked
                .invalidate()
                .await
                .map_err(|_| BffError::Unavailable)?;
            Some(token)
        }
        Err(StoreError::Unavailable) => return Err(BffError::Unavailable),
        Err(_) => {
            state
                .inner
                .store
                .delete_session(token_digest(&cookie))
                .await
                .map_err(|_| BffError::Unavailable)?;
            None
        }
    };
    // Local authority is committed invalid before revocation. A network failure cannot restore it.
    let revocation = match token {
        Some(token) if state.inner.oidc.revoke(&token).await.is_ok() => "revoked",
        Some(_) => "failed",
        None => "not_required",
    };
    let mut response=Json(json!({"status":"logged_out","revocation_status":revocation,"redirect_to":if identity {Some(state.inner.oidc.identity_logout())}else{None}})).into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        state.cookie(&state.inner.cookie, "", 0)?,
    );
    safe_headers(&mut response);
    Ok(response)
}
