//! Browser trust boundary. Production routes never include the integration security-check fixture.
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use identity_core::{
    config::{Config, Environment},
    security::{AeadKeyRing, Token, keyed_account_digest, normalize_email, token_digest},
};
use identity_store::{Dependencies, repository::Digest, security::BrowserSecurityStore};
use ipnet::IpNet;
use serde::de::{self, DeserializeOwned, MapAccess, SeqAccess, Visitor};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fmt,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundaryUnavailable;

#[derive(Clone, Copy, Debug)]
pub struct RequestId(pub Uuid);
#[derive(Clone, Copy, Debug)]
pub struct TrustedSource(pub IpAddr);

/// Safe code/message only. Values from parser/database/transport errors are never returned.
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub request_id: Uuid,
    pub retry_after: Option<u64>,
}
impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, request_id: Uuid) -> Self {
        Self {
            status,
            code,
            request_id,
            retry_after: None,
        }
    }
    fn unavailable(id: Uuid) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "DEPENDENCY_UNAVAILABLE",
            id,
        )
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let message = match self.code {
            "AUTH_CSRF_INVALID" => "安全验证失效，请重新获取后再试",
            "AUTH_ORIGIN_INVALID" => "请求来源无效",
            "REQUEST_TOO_LARGE" => "请求内容超出限制",
            "RATE_LIMITED" => "请求较频繁，请稍后重试",
            "RESOURCE_NOT_FOUND" => "请求的资源不存在",
            "INPUT_INVALID" => "输入无效，请检查后重试",
            "AUTH_SESSION_REQUIRED" => "请先登录再继续",
            "AUTH_INVALID_CREDENTIALS" => "凭证无效，请重试",
            "AUTH_ACCOUNT_DISABLED" => "凭证无效，请重试",
            "AUTH_EMAIL_UNVERIFIED" => "请先完成邮箱验证",
            "AUTH_CHALLENGE_INVALID" => "认证挑战无效，请重新开始",
            "AUTH_CHALLENGE_EXPIRED" => "认证挑战已过期，请重新开始",
            "AUTH_CHALLENGE_CONSUMED" => "认证挑战已使用或次数耗尽，请重新开始",
            "AUTH_FACTOR_INVALID" => "验证码或恢复码无效，请重试",
            "AUTH_FACTOR_REPLAYED" => "验证码已使用，请等待下一个验证码",
            "STATE_CONFLICT" => "当前状态不允许此操作，请刷新后重试",
            "AUTH_REAUTH_REQUIRED" => "请先完成近期重新认证",
            "ADMIN_STRONG_AUTH_REQUIRED" => "请先完成近期强认证",
            "AUTH_ACTION_INVALID" => "验证链接无效，请重新申请验证邮件",
            "AUTH_ACTION_EXPIRED" => "验证链接已过期，请重新申请验证邮件",
            "AUTH_ACTION_CONSUMED" => "验证链接已使用或已被新邮件替换，请使用最新邮件",
            _ => "服务暂不可用，请稍后重试",
        };
        let mut response = (
            self.status,
            Json(
                json!({"error":{"code":self.code,"message":message,"request_id":self.request_id}}),
            ),
        )
            .into_response();
        if let Some(seconds) = self.retry_after
            && let Ok(value) = HeaderValue::from_str(&seconds.to_string())
        {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        response
    }
}

/// Discriminated step-up response; no arbitrary frontend status or permission string.
#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequiredStrength {
    Password,
    Strong,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReauthMethod {
    Password,
    Totp,
    RecoveryCode,
    Passkey,
}
pub struct ReauthFailure {
    pub request_id: Uuid,
    pub required_strength: RequiredStrength,
    pub methods: Vec<ReauthMethod>,
}
impl IntoResponse for ReauthFailure {
    fn into_response(self) -> Response {
        (StatusCode::FORBIDDEN,Json(json!({"error":{"code":"AUTH_REAUTH_REQUIRED","message":"请先完成近期重新认证","request_id":self.request_id},"next":{"status":"reauth_required","required_strength":self.required_strength,"methods":self.methods}}))).into_response()
    }
}

#[derive(Clone)]
pub struct SecurityState(Arc<SecurityInner>);
struct SecurityInner {
    store: BrowserSecurityStore,
    redis: redis::Client,
    origin: String,
    session_cookie: &'static str,
    preauth_cookie: &'static str,
    secure: bool,
    proxies: Vec<IpNet>,
    limit_key: Zeroizing<[u8; 32]>,
    clock: Arc<dyn identity_core::clock::Clock>,
}
impl SecurityState {
    pub fn new(config: &Config, dependencies: Dependencies) -> Result<Self, &'static str> {
        Self::new_with_clock(
            config,
            dependencies,
            Arc::new(identity_core::clock::SystemClock),
        )
    }
    pub fn new_with_clock(
        config: &Config,
        dependencies: Dependencies,
        clock: Arc<dyn identity_core::clock::Clock>,
    ) -> Result<Self, &'static str> {
        let ring =
            AeadKeyRing::load_file(&config.encryption_keys_file, &config.active_encryption_kid)
                .map_err(|_| "invalid encryption key configuration")?;
        let limit_key = ring
            .derive_hmac_key("identity-rate-limit-v1")
            .map_err(|_| "invalid rate limit key configuration")?;
        let redis = redis::Client::open(config.redis_url.expose())
            .map_err(|_| "invalid Redis configuration")?;
        let secure =
            config.environment == Environment::Production || config.issuer.scheme() == "https";
        Ok(Self(Arc::new(SecurityInner {
            store: BrowserSecurityStore::new(dependencies.postgres),
            redis,
            origin: config.issuer.origin().ascii_serialization(),
            session_cookie: if secure {
                "__Host-identity"
            } else {
                "identity-dev"
            },
            preauth_cookie: if secure {
                "__Host-identity-preauth"
            } else {
                "identity-preauth-dev"
            },
            secure,
            proxies: config.trusted_proxy_cidrs.clone(),
            limit_key,
            clock,
        })))
    }
    pub fn cookie_names(&self) -> (&'static str, &'static str) {
        (self.0.session_cookie, self.0.preauth_cookie)
    }
    pub fn identity_digest(
        &self,
        headers: &HeaderMap,
    ) -> Result<Option<Digest>, BoundaryUnavailable> {
        cookie_digest(headers, self.0.session_cookie).map_err(|_| BoundaryUnavailable)
    }
    pub fn preauth_digest(
        &self,
        headers: &HeaderMap,
    ) -> Result<Option<Digest>, BoundaryUnavailable> {
        cookie_digest(headers, self.0.preauth_cookie).map_err(|_| BoundaryUnavailable)
    }
    pub fn cursor_key(&self) -> [u8; 32] {
        keyed_account_digest(
            &self.0.limit_key,
            "session-cursor-v1",
            "created_at_desc_id_desc",
        )
    }
    pub fn identity_cookie(&self, token: &str) -> Result<HeaderValue, BoundaryUnavailable> {
        self.cookie(self.0.session_cookie, token, 43_200)
    }
    pub fn preauth_cookie(&self, token: &str) -> Result<HeaderValue, BoundaryUnavailable> {
        self.cookie(self.0.preauth_cookie, token, 600)
    }
    pub fn clear_identity_cookie(&self) -> Result<HeaderValue, BoundaryUnavailable> {
        self.cookie(self.0.session_cookie, "", 0)
    }
    fn cookie(
        &self,
        name: &str,
        token: &str,
        age: u32,
    ) -> Result<HeaderValue, BoundaryUnavailable> {
        if age > 0 {
            token_hash(token).map_err(|_| BoundaryUnavailable)?;
        }
        HeaderValue::from_str(&format!(
            "{name}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={age}{}",
            if self.0.secure { "; Secure" } else { "" }
        ))
        .map_err(|_| BoundaryUnavailable)
    }
    pub fn origin(&self) -> &str {
        &self.0.origin
    }
    fn source(&self, request: &Request) -> Result<IpAddr, ApiError> {
        let id = request
            .extensions()
            .get::<RequestId>()
            .map_or_else(Uuid::new_v4, |id| id.0);
        let peer = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "INPUT_INVALID", id))?
            .0
            .ip();
        trusted_source(peer, request.headers(), &self.0.proxies)
            .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "INPUT_INVALID", id))
    }
    pub async fn check_limit(
        &self,
        policy: LimitPolicy,
        source: IpAddr,
        account: Option<&str>,
        challenge: Option<Uuid>,
    ) -> Result<LimitDecision, BoundaryUnavailable> {
        let mut budgets = Vec::new();
        let (ip_limit, seconds) = match policy {
            LimitPolicy::PasswordLogin => (30, 60),
            LimitPolicy::Mail => (20, 3600),
            LimitPolicy::Challenge => (30, 60),
            LimitPolicy::Preauth => (30, 60),
        };
        budgets.push((
            self.limit_key(&format!("{}:ip", policy.key()), &source.to_string()),
            ip_limit,
            seconds,
        ));
        if let Some(account) = account {
            let normalized = normalize_email(account).map_err(|_| BoundaryUnavailable)?;
            let (limit, seconds) = if matches!(policy, LimitPolicy::Mail) {
                (3, 3600)
            } else {
                (5, 60)
            };
            budgets.push((
                self.limit_key(&format!("{}:account", policy.key()), &normalized),
                limit,
                seconds,
            ));
        }
        if let Some(challenge) = challenge {
            budgets.push((
                self.limit_key(
                    &format!("{}:challenge", policy.key()),
                    &challenge.to_string(),
                ),
                5,
                300,
            ));
        }
        // One Lua invocation updates all budgets atomically; only HMAC digests are Redis keys.
        let script = redis::Script::new(LIMIT_SCRIPT);
        let mut invocation = script.prepare_invoke();
        for (key, _, _) in &budgets {
            invocation.key(key);
        }
        for (_, limit, seconds) in &budgets {
            invocation.arg(*limit).arg(*seconds);
        }
        let result = tokio::time::timeout(Duration::from_secs(2), async {
            let mut connection = self.0.redis.get_multiplexed_async_connection().await?;
            invocation.invoke_async::<Vec<i64>>(&mut connection).await
        })
        .await
        .map_err(|_| BoundaryUnavailable)?
        .map_err(|_| BoundaryUnavailable)?;
        if result.len() != 3 {
            return Err(BoundaryUnavailable);
        }
        if result[0] == 0 {
            Ok(LimitDecision::Allowed)
        } else {
            Ok(LimitDecision::Limited {
                mail_target: matches!(policy, LimitPolicy::Mail) && result[2] == 2,
                retry_after: u64::try_from(result[1].max(1)).map_err(|_| BoundaryUnavailable)?,
            })
        }
    }
    pub fn source_digest(&self, source: IpAddr) -> Digest {
        Digest::from_bytes(keyed_account_digest(
            &self.0.limit_key,
            "audit-source",
            &source.to_string(),
        ))
    }
    fn limit_key(&self, purpose: &str, value: &str) -> String {
        let digest = keyed_account_digest(&self.0.limit_key, purpose, value);
        format!(
            "identity:budget:{purpose}:{}",
            BASE64_URL_SAFE_NO_PAD.encode(digest)
        )
    }
}
#[derive(Clone, Copy, Debug)]
pub enum LimitPolicy {
    PasswordLogin,
    Mail,
    Challenge,
    Preauth,
}
impl LimitPolicy {
    fn key(self) -> &'static str {
        match self {
            Self::PasswordLogin => "login",
            Self::Mail => "mail",
            Self::Challenge => "mfa",
            Self::Preauth => "preauth",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitDecision {
    Allowed,
    Limited { retry_after: u64, mail_target: bool },
}
const LIMIT_SCRIPT: &str = r#"
local retry=0
local dimension=0
local iplimited=false
for i,key in ipairs(KEYS) do
 local maximum=tonumber(ARGV[(i-1)*2+1])
 local seconds=tonumber(ARGV[(i-1)*2+2])
 local count=redis.call('INCR',key)
 if count==1 then redis.call('EXPIRE',key,seconds) end
 if count>maximum then
  local ttl=redis.call('TTL',key)
  if ttl>retry then retry=ttl;dimension=i end
  if i==1 then iplimited=true end
 end
end
if iplimited then dimension=1 end
if retry>0 then return {1,retry,dimension} end
return {0,0,0}
"#;

/// Peers outside the whitelist cannot affect source with any forwarding header.
/// For trusted peers, peel trusted hops from right to left and stop at the first untrusted hop.
pub fn trusted_source(
    peer: IpAddr,
    headers: &HeaderMap,
    proxies: &[IpNet],
) -> Result<IpAddr, BoundaryUnavailable> {
    let trusted = |ip: IpAddr| proxies.iter().any(|cidr| cidr.contains(&ip));
    if !trusted(peer) {
        return Ok(peer);
    }
    let mut current = peer;
    let mut forwarded = Vec::new();
    for value in headers.get_all("x-forwarded-for") {
        let text = value.to_str().map_err(|_| BoundaryUnavailable)?;
        for item in text.split(',') {
            if forwarded.len() >= 32 {
                return Err(BoundaryUnavailable);
            }
            forwarded.push(
                item.trim()
                    .parse::<IpAddr>()
                    .map_err(|_| BoundaryUnavailable)?,
            );
        }
    }
    for hop in forwarded.into_iter().rev() {
        if !trusted(current) {
            break;
        }
        current = hop;
    }
    Ok(current)
}

fn unique_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>, ()> {
    let mut values = headers.get_all(name).iter();
    let value = values.next();
    if values.next().is_some() {
        return Err(());
    }
    value
        .map(|value| value.to_str().map_err(|_| ()))
        .transpose()
}
fn cookie_digest(headers: &HeaderMap, name: &str) -> Result<Option<Digest>, ()> {
    let mut value = None;
    for cookie in headers.get_all(header::COOKIE) {
        for pair in cookie.to_str().map_err(|_| ())?.split(';') {
            let Some((key, raw)) = pair.trim().split_once('=') else {
                continue;
            };
            if key == name {
                if value.is_some() {
                    return Err(());
                }
                value = Some(raw);
            }
        }
    }
    value.map(token_hash).transpose()
}
fn token_hash(token: &str) -> Result<Digest, ()> {
    if token.len() != 43 {
        return Err(());
    }
    let decoded = BASE64_URL_SAFE_NO_PAD.decode(token).map_err(|_| ())?;
    if decoded.len() != 32 {
        return Err(());
    }
    Ok(Digest::from_bytes(token_digest(token)))
}

async fn csrf(State(state): State<SecurityState>, request: Request) -> Result<Response, ApiError> {
    let id = request
        .extensions()
        .get::<RequestId>()
        .map_or_else(Uuid::new_v4, |id| id.0);
    let now = state.0.clock.now();
    let source = state.source(&request)?;
    limit(&state, LimitPolicy::Preauth, source, None, None, id).await?;
    let csrf = Token::generate().map_err(|_| ApiError::unavailable(id))?;
    let csrf_hash = token_hash(csrf.expose()).map_err(|_| ApiError::unavailable(id))?;
    let session = cookie_digest(request.headers(), state.0.session_cookie)
        .map_err(|_| ApiError::new(StatusCode::FORBIDDEN, "AUTH_CSRF_INVALID", id))?;
    let mut response = Json(json!({"csrf_token":csrf.expose()})).into_response();
    let rotated = if let Some(session) = session {
        state
            .0
            .store
            .rotate_session_csrf(session, csrf_hash, now)
            .await
            .map_err(|_| ApiError::unavailable(id))?
    } else {
        false
    };
    let old = cookie_digest(request.headers(), state.0.preauth_cookie)
        .map_err(|_| ApiError::new(StatusCode::FORBIDDEN, "AUTH_CSRF_INVALID", id))?;
    let kept = if !rotated {
        if let Some(old) = old {
            state
                .0
                .store
                .rotate_preauth_csrf(old, csrf_hash, now)
                .await
                .map_err(|_| ApiError::unavailable(id))?
        } else {
            false
        }
    } else {
        true
    };
    if !kept {
        let preauth = Token::generate().map_err(|_| ApiError::unavailable(id))?;
        let old = cookie_digest(request.headers(), state.0.preauth_cookie)
            .map_err(|_| ApiError::new(StatusCode::FORBIDDEN, "AUTH_CSRF_INVALID", id))?;
        state
            .0
            .store
            .replace_preauth(
                old,
                token_hash(preauth.expose()).map_err(|_| ApiError::unavailable(id))?,
                csrf_hash,
                now,
            )
            .await
            .map_err(|_| ApiError::unavailable(id))?;
        let cookie = format!(
            "{}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age=600{}",
            state.0.preauth_cookie,
            preauth.expose(),
            if state.0.secure { "; Secure" } else { "" }
        );
        response.headers_mut().append(
            header::SET_COOKIE,
            HeaderValue::from_str(&cookie).map_err(|_| ApiError::unavailable(id))?,
        );
    }
    if session.is_some() && !rotated {
        let clear = format!(
            "{}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{}",
            state.0.session_cookie,
            if state.0.secure { "; Secure" } else { "" }
        );
        response.headers_mut().append(
            header::SET_COOKIE,
            HeaderValue::from_str(&clear).map_err(|_| ApiError::unavailable(id))?,
        );
    }
    Ok(response)
}

async fn limit(
    state: &SecurityState,
    policy: LimitPolicy,
    source: IpAddr,
    account: Option<&str>,
    challenge: Option<Uuid>,
    id: Uuid,
) -> Result<(), ApiError> {
    match state
        .check_limit(policy, source, account, challenge)
        .await
        .map_err(|_| ApiError::unavailable(id))?
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

/// Wraps real routes with security. Test-only protected routes are supplied by tests, never production main.
pub fn security_router(state: SecurityState, protected: Router) -> Router {
    let csrf_route = Router::new()
        .route("/api/v1/auth/csrf", get(csrf))
        .with_state(state.clone());
    protected
        .merge(csrf_route)
        .fallback(safe_not_found)
        .method_not_allowed_fallback(safe_method_not_allowed)
        .layer(middleware::from_fn_with_state(state, browser_boundary))
}

async fn safe_not_found(request: Request) -> Response {
    let id = request
        .extensions()
        .get::<RequestId>()
        .map_or_else(Uuid::new_v4, |id| id.0);
    ApiError::new(StatusCode::NOT_FOUND, "RESOURCE_NOT_FOUND", id).into_response()
}
async fn safe_method_not_allowed(request: Request) -> Response {
    let id = request
        .extensions()
        .get::<RequestId>()
        .map_or_else(Uuid::new_v4, |id| id.0);
    ApiError::new(StatusCode::METHOD_NOT_ALLOWED, "INPUT_INVALID", id).into_response()
}

async fn browser_boundary(
    State(state): State<SecurityState>,
    mut request: Request,
    next: Next,
) -> Response {
    let id = Uuid::new_v4();
    request.extensions_mut().insert(RequestId(id));
    let path = request.uri().path().to_string();
    let method = request.method().clone();
    let result = async {
        let source = state.source(&request)?;
        request.extensions_mut().insert(TrustedSource(source));
        if path.starts_with("/api/v1/")
            && !matches!(method, Method::GET | Method::HEAD | Method::OPTIONS)
        {
            if unique_header(request.headers(), "origin")
                .map_err(|_| ApiError::new(StatusCode::FORBIDDEN, "AUTH_ORIGIN_INVALID", id))?
                != Some(state.origin())
            {
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "AUTH_ORIGIN_INVALID",
                    id,
                ));
            }
            let csrf = unique_header(request.headers(), "x-csrf-token")
                .map_err(|_| ApiError::new(StatusCode::FORBIDDEN, "AUTH_CSRF_INVALID", id))?
                .ok_or_else(|| ApiError::new(StatusCode::FORBIDDEN, "AUTH_CSRF_INVALID", id))?;
            let csrf = token_hash(csrf)
                .map_err(|_| ApiError::new(StatusCode::FORBIDDEN, "AUTH_CSRF_INVALID", id))?;
            let session = cookie_digest(request.headers(), state.0.session_cookie)
                .map_err(|_| ApiError::new(StatusCode::FORBIDDEN, "AUTH_CSRF_INVALID", id))?;
            let preauth = cookie_digest(request.headers(), state.0.preauth_cookie)
                .map_err(|_| ApiError::new(StatusCode::FORBIDDEN, "AUTH_CSRF_INVALID", id))?;
            if !state
                .0
                .store
                .valid_csrf(session, preauth, csrf, state.0.clock.now())
                .await
                .map_err(|_| ApiError::unavailable(id))?
            {
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "AUTH_CSRF_INVALID",
                    id,
                ));
            }
        }
        let maximum = if path.starts_with("/api/v1/auth/")
            || path.contains("/passkeys")
            || path.contains("/mfa/")
            || path.contains("/reauth/")
        {
            65_536
        } else {
            32_768
        };
        let (parts, body) = request.into_parts();
        let bytes = to_bytes(body, maximum)
            .await
            .map_err(|_| ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "REQUEST_TOO_LARGE", id))?;
        let mut request = Request::from_parts(parts, Body::from(bytes));
        request.extensions_mut().insert(RequestId(id));
        Ok(next.run(request).await)
    }
    .await;
    let mut response = match result {
        Ok(response) => response,
        Err(error) => error.into_response(),
    };
    let headers = response.headers_mut();
    headers.insert(
        "x-request-id",
        HeaderValue::from_str(&id.to_string())
            .unwrap_or_else(|_| HeaderValue::from_static("invalid")),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("content-security-policy",HeaderValue::from_static("default-src 'self'; script-src 'self'; frame-ancestors 'none'; object-src 'none'; base-uri 'none'"));
    response
}

/// Strict JSON helper gives handler parsing failures the same safe envelope as middleware.
pub async fn read_json<T: DeserializeOwned>(request: Request) -> Result<T, ApiError> {
    let id = request
        .extensions()
        .get::<RequestId>()
        .map_or_else(Uuid::new_v4, |id| id.0);
    if unique_header(request.headers(), "content-type")
        .ok()
        .flatten()
        .is_none_or(|value| {
            !value
                .split(';')
                .next()
                .is_some_and(|kind| kind.trim() == "application/json")
        })
    {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "INPUT_INVALID",
            id,
        ));
    }
    let bytes = to_bytes(request.into_body(), 65_536)
        .await
        .map_err(|_| ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "REQUEST_TOO_LARGE", id))?;
    let strict: StrictJson = serde_json::from_slice(&bytes)
        .map_err(|_| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "INPUT_INVALID", id))?;
    serde_json::from_value(strict.0)
        .map_err(|_| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "INPUT_INVALID", id))
}

/// JSON duplicate members are rejected recursively before DTO deserialization can discard information.
struct StrictJson(Value);
impl<'de> serde::Deserialize<'de> for StrictJson {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StrictVisitor;
        impl<'de> Visitor<'de> for StrictVisitor {
            type Value = StrictJson;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("JSON with unique object members")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(StrictJson(Value::Bool(v)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(StrictJson(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(StrictJson(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|number| StrictJson(Value::Number(number)))
                    .ok_or_else(|| E::custom("invalid JSON number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StrictJson(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(StrictJson(v.into()))
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictJson(Value::Null))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictJson(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element::<StrictJson>()? {
                    values.push(value.0);
                }
                Ok(StrictJson(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = BTreeMap::new();
                while let Some((key, value)) = map.next_entry::<String, StrictJson>()? {
                    if values.insert(key, value.0).is_some() {
                        return Err(de::Error::custom("duplicate JSON member"));
                    }
                }
                Ok(StrictJson(Value::Object(values.into_iter().collect())))
            }
        }
        deserializer.deserialize_any(StrictVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untrusted_forwarding_headers_cannot_override_peer() {
        let peer = IpAddr::from([203, 0, 113, 8]);
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("127.0.0.1, invalid-sentinel"),
        );
        assert_eq!(trusted_source(peer, &headers, &[]), Ok(peer));
    }
    #[test]
    fn trusted_proxy_chain_stops_at_nearest_untrusted_hop() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("198.51.100.77, 203.0.113.45, 10.0.0.2"),
        );
        let proxies: Vec<IpNet> = ["10.0.0.0/8"]
            .into_iter()
            .filter_map(|text| text.parse().ok())
            .collect();
        assert_eq!(
            trusted_source(IpAddr::from([10, 0, 0, 3]), &headers, &proxies),
            Ok(IpAddr::from([203, 0, 113, 45]))
        );
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("invalid-secret"),
        );
        assert!(trusted_source(IpAddr::from([10, 0, 0, 3]), &headers, &proxies).is_err());
    }
    #[test]
    fn duplicate_members_are_rejected_before_json_dto_parsing() {
        for text in [
            r#"{"email":"first","email":"second"}"#,
            r#"{"credential":{"id":"first","id":"second"}}"#,
        ] {
            assert!(serde_json::from_str::<StrictJson>(text).is_err());
        }
        assert!(
            serde_json::from_str::<StrictJson>(
                r#"{"email":"person@example.test","nested":[true,1,null]}"#
            )
            .is_ok()
        );
    }
    #[test]
    fn duplicate_sensitive_cookie_or_header_is_invalid() {
        let mut headers = HeaderMap::new();
        headers.append(
            "origin",
            HeaderValue::from_static("https://identity.example"),
        );
        headers.append(
            "origin",
            HeaderValue::from_static("https://attacker.example"),
        );
        assert!(unique_header(&headers, "origin").is_err());
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("identity-dev=a; identity-dev=b"),
        );
        assert!(cookie_digest(&headers, "identity-dev").is_err());
    }
}
