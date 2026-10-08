//! Management endpoints check live identity and administrator strength independently of browser UI.
use crate::{
    accounts::AuthAppState,
    security::{
        ApiError, LimitDecision, LimitPolicy, ReauthFailure, ReauthMethod, RequestId,
        RequiredStrength, TrustedSource, read_json,
    },
};
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
};
use identity_core::{config::Environment, oauth::Scope};
use identity_store::{
    admin::{AdminContext, AdminError, ClientCreateInput, ClientUpdateInput},
    sessions::{MeView, SessionError},
};
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::json;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UserStatusRequest {
    status: UserStatus,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum UserStatus {
    Active,
    Disabled,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MemberRequest {
    user_id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientCreateRequest {
    name: String,
    allowed_scopes: Vec<String>,
    redirect_uris: Vec<String>,
    post_logout_redirect_uris: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientUpdateRequest {
    #[serde(default, deserialize_with = "present")]
    name: Option<String>,
    #[serde(default, deserialize_with = "present")]
    enabled: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    allowed_scopes: Option<Vec<String>>,
    #[serde(default, deserialize_with = "present")]
    redirect_uris: Option<Vec<String>>,
    #[serde(default, deserialize_with = "present")]
    post_logout_redirect_uris: Option<Vec<String>>,
}
// Missing is permitted for PATCH; explicit JSON null is not a valid string/bool/array.
fn present<'de, D: Deserializer<'de>, T: DeserializeOwned>(
    value: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(value).map(Some)
}

pub fn admin_routes(state: AuthAppState) -> Router {
    Router::new()
        .route("/api/v1/admin/users", get(users))
        .route("/api/v1/admin/users/{id}", get(user))
        .route("/api/v1/admin/users/{id}/status", patch(user_status))
        .route(
            "/api/v1/admin/users/{id}/revoke-sessions",
            post(revoke_sessions),
        )
        .route("/api/v1/admin/clients", get(clients).post(create_client))
        .route(
            "/api/v1/admin/clients/{id}",
            get(client).patch(update_client),
        )
        .route(
            "/api/v1/admin/clients/{id}/rotate-secret",
            post(rotate_secret),
        )
        .route("/api/v1/admin/members", get(members).post(add_member))
        .route("/api/v1/admin/members/{user_id}", delete(remove_member))
        .route("/api/v1/admin/audit-events", get(audit_events))
        .with_state(state)
}
fn request_id(request: &Request) -> Uuid {
    request
        .extensions()
        .get::<RequestId>()
        .map_or_else(Uuid::new_v4, |id| id.0)
}
fn unavailable(id: Uuid) -> ApiError {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "DEPENDENCY_UNAVAILABLE",
        id,
    )
}
fn invalid(id: Uuid) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "INPUT_INVALID", id)
}
fn session_required(id: Uuid) -> ApiError {
    ApiError::new(StatusCode::UNAUTHORIZED, "AUTH_SESSION_REQUIRED", id)
}
fn failure(error: AdminError, id: Uuid) -> Response {
    let (status, code) = match error {
        AdminError::Forbidden => (StatusCode::FORBIDDEN, "ADMIN_FORBIDDEN"),
        AdminError::StrongRequired => (StatusCode::FORBIDDEN, "AUTH_REAUTH_REQUIRED"),
        AdminError::LastAdministrator => (StatusCode::CONFLICT, "ADMIN_LAST_MEMBER"),
        AdminError::NotFound => (StatusCode::NOT_FOUND, "RESOURCE_NOT_FOUND"),
        AdminError::InvalidInput => (StatusCode::BAD_REQUEST, "INPUT_INVALID"),
        AdminError::TargetIneligible => (StatusCode::CONFLICT, "ADMIN_TARGET_INELIGIBLE"),
        AdminError::StateConflict => (StatusCode::CONFLICT, "STATE_CONFLICT"),
        _ => (StatusCode::SERVICE_UNAVAILABLE, "DEPENDENCY_UNAVAILABLE"),
    };
    ApiError::new(status, code, id).into_response()
}
fn strong_methods(me: &MeView) -> Vec<ReauthMethod> {
    let mut methods = Vec::new();
    if me.security.totp_enabled {
        methods.push(ReauthMethod::Totp);
    }
    if me.security.recovery_codes_remaining > 0 {
        methods.push(ReauthMethod::RecoveryCode);
    }
    if me.security.passkey_count > 0 {
        methods.push(ReauthMethod::Passkey);
    }
    methods
}
async fn store_failure(state: &AuthAppState, error: AdminError, context: AdminContext) -> Response {
    if matches!(error, AdminError::StrongRequired) {
        return match state.sessions.me(context.session_hash).await {
            Ok(Some(me)) if me.security.is_admin && !me.security.admin_binding_only => {
                ReauthFailure {
                    request_id: context.request_id,
                    required_strength: RequiredStrength::Strong,
                    methods: strong_methods(&me),
                }
                .into_response()
            }
            Ok(Some(_)) => failure(AdminError::Forbidden, context.request_id),
            Ok(None) => session_required(context.request_id).into_response(),
            Err(_) => unavailable(context.request_id).into_response(),
        };
    }
    failure(error, context.request_id)
}
struct AuthorizationRequest {
    session_hash: identity_store::repository::Digest,
    id: Uuid,
    source: std::net::IpAddr,
}
fn authorization_request(
    state: &AuthAppState,
    request: &Request,
) -> Result<AuthorizationRequest, ApiError> {
    let id = request_id(request);
    let session_hash = state
        .security
        .identity_digest(request.headers())
        .map_err(|_| session_required(id))?
        .ok_or_else(|| session_required(id))?;
    let source = request
        .extensions()
        .get::<TrustedSource>()
        .map(|source| source.0)
        .ok_or_else(|| unavailable(id))?;
    Ok(AuthorizationRequest {
        session_hash,
        id,
        source,
    })
}
async fn authorize(
    state: &AuthAppState,
    request: Result<AuthorizationRequest, ApiError>,
) -> Result<AdminContext, Box<Response>> {
    let AuthorizationRequest {
        session_hash,
        id,
        source,
    } = request.map_err(|error| Box::new(error.into_response()))?;
    let me = state
        .sessions
        .me(session_hash)
        .await
        .map_err(|error| match error {
            SessionError::Unavailable => Box::new(unavailable(id).into_response()),
            _ => Box::new(session_required(id).into_response()),
        })?
        .ok_or_else(|| Box::new(session_required(id).into_response()))?;
    if !me.security.is_admin || me.security.admin_binding_only {
        return Err(Box::new(failure(AdminError::Forbidden, id)));
    }
    let context = AdminContext {
        session_hash,
        request_id: id,
        source_hash: state.security.source_digest(source),
    };
    state.admin.authorize(context).await.map_err(|error| {
        if matches!(error, AdminError::StrongRequired) {
            Box::new(
                ReauthFailure {
                    request_id: id,
                    required_strength: RequiredStrength::Strong,
                    methods: strong_methods(&me),
                }
                .into_response(),
            )
        } else {
            Box::new(failure(error, id))
        }
    })?;
    let account = me.user.id.to_string();
    match state
        .security
        .check_limit(LimitPolicy::Admin, source, Some(&account), None)
        .await
        .map_err(|_| Box::new(unavailable(id).into_response()))?
    {
        LimitDecision::Allowed => {}
        LimitDecision::Limited { retry_after, .. } => {
            return Err(Box::new(
                ApiError {
                    status: StatusCode::TOO_MANY_REQUESTS,
                    code: "RATE_LIMITED",
                    request_id: id,
                    retry_after: Some(retry_after),
                }
                .into_response(),
            ));
        }
    }
    Ok(context)
}
fn page(request: &Request, id: Uuid) -> Result<(u32, Option<String>), ApiError> {
    let mut limit = 20;
    let mut limit_seen = false;
    let mut cursor = None;
    let query = request.uri().query().unwrap_or("");
    if query.len() > 4096 || !valid_percent(query) {
        return Err(invalid(id));
    }
    for (name, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match name.as_ref() {
            "limit" if !limit_seen => {
                limit_seen = true;
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(invalid(id));
                }
                limit = value.parse().map_err(|_| invalid(id))?;
            }
            "cursor" if cursor.is_none() && !value.is_empty() && value.len() <= 2048 => {
                cursor = Some(value.into_owned())
            }
            _ => return Err(invalid(id)),
        }
    }
    if !(1..=100).contains(&limit) {
        return Err(invalid(id));
    }
    Ok((limit, cursor))
}
fn valid_percent(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len()
                || !bytes[i + 1].is_ascii_hexdigit()
                || !bytes[i + 2].is_ascii_hexdigit()
            {
                return false;
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    true
}
fn target(request: &Request, trailing: bool, id: Uuid) -> Result<Uuid, ApiError> {
    if request.uri().query().is_some_and(|value| !value.is_empty()) {
        return Err(invalid(id));
    }
    let mut segments = request.uri().path().rsplit('/');
    let value = if trailing {
        segments.nth(1)
    } else {
        segments.next()
    };
    value
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| invalid(id))
}
async fn empty(request: Request, id: Uuid) -> Result<(), ApiError> {
    let bytes = to_bytes(request.into_body(), 32768)
        .await
        .map_err(|_| ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "REQUEST_TOO_LARGE", id))?;
    if !bytes.is_empty() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "INPUT_INVALID",
            id,
        ));
    }
    Ok(())
}
fn scopes(values: &[String], id: Uuid) -> Result<Vec<Scope>, ApiError> {
    let result = values
        .iter()
        .map(|value| match value.as_str() {
            "openid" => Ok(Scope::OpenId),
            "profile" => Ok(Scope::Profile),
            "email" => Ok(Scope::Email),
            _ => Err(invalid(id)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if result.is_empty()
        || result.len() > 3
        || !result.contains(&Scope::OpenId)
        || result
            .iter()
            .enumerate()
            .any(|(i, value)| result[..i].contains(value))
    {
        return Err(invalid(id));
    }
    Ok(result)
}
async fn users(State(state): State<AuthAppState>, request: Request) -> Result<Response, ApiError> {
    let context = match authorize(&state, authorization_request(&state, &request)).await {
        Ok(context) => context,
        Err(response) => return Ok(*response),
    };
    let (limit, cursor) = page(&request, context.request_id)?;
    Ok(
        match state
            .admin
            .users(
                context,
                limit,
                cursor.as_deref(),
                &state.security.cursor_key(),
            )
            .await
        {
            Ok(view) => Json(view).into_response(),
            Err(error) => store_failure(&state, error, context).await,
        },
    )
}
async fn user(State(state): State<AuthAppState>, request: Request) -> Result<Response, ApiError> {
    let context = match authorize(&state, authorization_request(&state, &request)).await {
        Ok(context) => context,
        Err(response) => return Ok(*response),
    };
    let id = target(&request, false, context.request_id)?;
    Ok(match state.admin.user(context, id).await {
        Ok(view) => Json(view).into_response(),
        Err(error) => store_failure(&state, error, context).await,
    })
}
async fn user_status(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let context = match authorize(&state, authorization_request(&state, &request)).await {
        Ok(context) => context,
        Err(response) => return Ok(*response),
    };
    let id = target(&request, true, context.request_id)?;
    let input = read_json::<UserStatusRequest>(request).await?;
    Ok(
        match state
            .admin
            .set_user_status(context, id, matches!(input.status, UserStatus::Active))
            .await
        {
            Ok(view) => Json(view).into_response(),
            Err(error) => store_failure(&state, error, context).await,
        },
    )
}
async fn revoke_sessions(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let context = match authorize(&state, authorization_request(&state, &request)).await {
        Ok(context) => context,
        Err(response) => return Ok(*response),
    };
    let id = target(&request, true, context.request_id)?;
    empty(request, context.request_id).await?;
    Ok(match state.admin.revoke_user_sessions(context, id).await {
        Ok(count) => {
            Json(json!({"status":"revoked","revoked_session_count":count})).into_response()
        }
        Err(error) => store_failure(&state, error, context).await,
    })
}
async fn clients(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let context = match authorize(&state, authorization_request(&state, &request)).await {
        Ok(context) => context,
        Err(response) => return Ok(*response),
    };
    let (limit, cursor) = page(&request, context.request_id)?;
    Ok(
        match state
            .admin
            .clients(
                context,
                limit,
                cursor.as_deref(),
                &state.security.cursor_key(),
            )
            .await
        {
            Ok(view) => Json(view).into_response(),
            Err(error) => store_failure(&state, error, context).await,
        },
    )
}
async fn client(State(state): State<AuthAppState>, request: Request) -> Result<Response, ApiError> {
    let context = match authorize(&state, authorization_request(&state, &request)).await {
        Ok(context) => context,
        Err(response) => return Ok(*response),
    };
    let id = target(&request, false, context.request_id)?;
    Ok(match state.admin.client(context, id).await {
        Ok(view) => Json(view).into_response(),
        Err(error) => store_failure(&state, error, context).await,
    })
}
async fn create_client(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let context = match authorize(&state, authorization_request(&state, &request)).await {
        Ok(context) => context,
        Err(response) => return Ok(*response),
    };
    if request.uri().query().is_some_and(|query| !query.is_empty()) {
        return Err(invalid(context.request_id));
    }
    let input = read_json::<ClientCreateRequest>(request).await?;
    let allowed_scopes = scopes(&input.allowed_scopes, context.request_id)?;
    Ok(match state.admin.create_client(context,&ClientCreateInput{name:input.name,allowed_scopes,redirect_uris:input.redirect_uris,post_logout_redirect_uris:input.post_logout_redirect_uris,production:state.inner.environment==Environment::Production}).await{
        Ok(created)=>(StatusCode::CREATED,Json(json!({"client":created.client,"client_secret":created.client_secret.expose(),"shown_once":true}))).into_response(),Err(error)=>store_failure(&state,error,context).await})
}
async fn update_client(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let context = match authorize(&state, authorization_request(&state, &request)).await {
        Ok(context) => context,
        Err(response) => return Ok(*response),
    };
    let id = target(&request, false, context.request_id)?;
    let input = read_json::<ClientUpdateRequest>(request).await?;
    if input.name.is_none()
        && input.enabled.is_none()
        && input.allowed_scopes.is_none()
        && input.redirect_uris.is_none()
        && input.post_logout_redirect_uris.is_none()
    {
        return Err(invalid(context.request_id));
    }
    let allowed_scopes = input
        .allowed_scopes
        .as_deref()
        .map(|values| scopes(values, context.request_id))
        .transpose()?;
    Ok(
        match state
            .admin
            .update_client(
                context,
                id,
                &ClientUpdateInput {
                    name: input.name,
                    enabled: input.enabled,
                    allowed_scopes,
                    redirect_uris: input.redirect_uris,
                    post_logout_redirect_uris: input.post_logout_redirect_uris,
                    production: state.inner.environment == Environment::Production,
                },
            )
            .await
        {
            Ok(view) => Json(view).into_response(),
            Err(error) => store_failure(&state, error, context).await,
        },
    )
}
async fn rotate_secret(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let context = match authorize(&state, authorization_request(&state, &request)).await {
        Ok(context) => context,
        Err(response) => return Ok(*response),
    };
    let id = target(&request, true, context.request_id)?;
    empty(request, context.request_id).await?;
    Ok(match state.admin.rotate_client_secret(context,id).await{Ok(created)=>Json(json!({"client_id":created.client.client_id,"client_secret":created.client_secret.expose(),"shown_once":true})).into_response(),Err(error)=>store_failure(&state,error,context).await})
}
async fn members(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let context = match authorize(&state, authorization_request(&state, &request)).await {
        Ok(context) => context,
        Err(response) => return Ok(*response),
    };
    let (limit, cursor) = page(&request, context.request_id)?;
    Ok(
        match state
            .admin
            .members(
                context,
                limit,
                cursor.as_deref(),
                &state.security.cursor_key(),
            )
            .await
        {
            Ok(view) => Json(view).into_response(),
            Err(error) => store_failure(&state, error, context).await,
        },
    )
}
async fn add_member(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let context = match authorize(&state, authorization_request(&state, &request)).await {
        Ok(context) => context,
        Err(response) => return Ok(*response),
    };
    if request.uri().query().is_some_and(|query| !query.is_empty()) {
        return Err(invalid(context.request_id));
    }
    let input = read_json::<MemberRequest>(request).await?;
    Ok(
        match state.admin.grant_member(context, input.user_id).await {
            Ok(view) => (StatusCode::CREATED, Json(view)).into_response(),
            Err(error) => store_failure(&state, error, context).await,
        },
    )
}
async fn remove_member(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let context = match authorize(&state, authorization_request(&state, &request)).await {
        Ok(context) => context,
        Err(response) => return Ok(*response),
    };
    let id = target(&request, false, context.request_id)?;
    empty(request, context.request_id).await?;
    Ok(match state.admin.remove_member(context, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => store_failure(&state, error, context).await,
    })
}
async fn audit_events(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let context = match authorize(&state, authorization_request(&state, &request)).await {
        Ok(context) => context,
        Err(response) => return Ok(*response),
    };
    let (limit, cursor) = page(&request, context.request_id)?;
    Ok(
        match state
            .admin
            .audit_events(
                context,
                limit,
                cursor.as_deref(),
                &state.security.cursor_key(),
            )
            .await
        {
            Ok(view) => Json(view).into_response(),
            Err(error) => store_failure(&state, error, context).await,
        },
    )
}

/// Bootstrap membership grants only factor binding authority until a real factor is configured.
pub(crate) async fn binding_boundary(
    State(state): State<AuthAppState>,
    request: Request,
    next: axum::middleware::Next,
) -> Response {
    let path = request.uri().path();
    if path.starts_with("/health/")
        || path.starts_with("/api/v1/admin/")
        || matches!(path, "/oauth/token" | "/oauth/introspect" | "/oauth/revoke")
        || binding_route(request.method(), path)
    {
        return next.run(request).await;
    }
    let id = request_id(&request);
    let hash = match state.security.identity_digest(request.headers()) {
        Ok(Some(hash)) => hash,
        Ok(None) => return next.run(request).await,
        Err(_) => return session_required(id).into_response(),
    };
    match state.sessions.me(hash).await {
        Ok(Some(me)) if me.security.admin_binding_only => {
            if path.starts_with("/oauth/") {
                crate::oauth::local_error(StatusCode::FORBIDDEN)
            } else {
                failure(AdminError::Forbidden, id)
            }
        }
        Ok(_) => next.run(request).await,
        Err(_) => unavailable(id).into_response(),
    }
}
fn binding_route(method: &axum::http::Method, path: &str) -> bool {
    use axum::http::Method;
    matches!(
        (method, path),
        (
            &Method::GET,
            "/api/v1/auth/csrf"
                | "/api/v1/me"
                | "/api/v1/me/passkeys"
                | "/.well-known/openid-configuration"
                | "/oauth/jwks"
                | "/oauth/logout"
        ) | (
            &Method::POST,
            "/api/v1/auth/login/password"
                | "/api/v1/auth/passkeys/options"
                | "/api/v1/auth/passkeys/verify"
                | "/api/v1/auth/mfa/totp/verify"
                | "/api/v1/auth/mfa/recovery/verify"
                | "/api/v1/auth/logout"
                | "/api/v1/auth/logout-all"
                | "/api/v1/auth/password-reset/request"
                | "/api/v1/auth/password-reset/confirm"
                | "/api/v1/auth/email-verification/request"
                | "/api/v1/auth/email-verification/confirm"
                | "/api/v1/me/reauth/password"
                | "/api/v1/me/reauth/totp"
                | "/api/v1/me/reauth/passkeys/options"
                | "/api/v1/me/reauth/passkeys/verify"
                | "/api/v1/me/mfa/totp/enrollment"
                | "/api/v1/me/mfa/totp/enrollment/confirm"
                | "/api/v1/me/passkeys/registration/options"
                | "/api/v1/me/passkeys/registration/verify"
                | "/oauth/logout"
                | "/oauth/logout/confirm"
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Method};
    use identity_store::sessions::{SecurityView, SessionView, UserView};
    use time::OffsetDateTime;
    #[test]
    fn pagination_rejects_ambiguous_unknown_and_malformed_parameters()
    -> Result<(), Box<dyn std::error::Error>> {
        let id = Uuid::new_v4();
        for query in [
            "limit=0",
            "limit=101",
            "limit=+2",
            "limit=2&limit=3",
            "cursor=one&cursor=two",
            "cursor=",
            "unknown=value",
            "limit=%GG",
            "cursor=%",
        ] {
            let request = Request::builder()
                .uri(format!("/api/v1/admin/users?{query}"))
                .body(Body::empty())?;
            assert!(page(&request, id).is_err());
        }
        let request = Request::builder()
            .uri("/api/v1/admin/users?limit=7&cursor=valid-input")
            .body(Body::empty())?;
        let (limit, cursor) = page(&request, id).map_err(|_| "valid page input rejected")?;
        assert_eq!(limit, 7);
        assert_eq!(cursor.as_deref(), Some("valid-input"));
        Ok(())
    }
    #[test]
    fn management_dtos_reject_mass_assignment_null_and_unknown_values() {
        for input in [
            r#"{"name":null}"#,
            r#"{"enabled":null}"#,
            r#"{"allowed_scopes":null}"#,
            r#"{"client_id":"attacker"}"#,
            r#"{"client_secret":"secret"}"#,
        ] {
            assert!(serde_json::from_str::<ClientUpdateRequest>(input).is_err());
        }
        assert!(serde_json::from_str::<ClientUpdateRequest>(r#"{"enabled":false}"#).is_ok());
        assert!(serde_json::from_str::<UserStatusRequest>(r#"{"status":"deleted"}"#).is_err());
        assert!(serde_json::from_str::<MemberRequest>(r#"{"user_id":"not-a-user"}"#).is_err());
        let id = Uuid::new_v4();
        for values in [
            vec!["openid".into(), "openid".into()],
            vec!["email".into()],
            vec!["admin".into()],
        ] {
            assert!(scopes(&values, id).is_err());
        }
        assert!(scopes(&["openid".into(), "profile".into(), "email".into()], id).is_ok());
    }
    #[test]
    fn bootstrap_allowlist_preserves_binding_and_rejects_other_account_authority() {
        for (method, path) in [
            (Method::GET, "/api/v1/me"),
            (Method::GET, "/api/v1/auth/csrf"),
            (Method::POST, "/api/v1/me/reauth/password"),
            (Method::POST, "/api/v1/me/mfa/totp/enrollment"),
            (Method::POST, "/api/v1/me/mfa/totp/enrollment/confirm"),
            (Method::POST, "/api/v1/me/passkeys/registration/verify"),
            (Method::POST, "/api/v1/auth/logout"),
        ] {
            assert!(binding_route(&method, path));
        }
        for (method, path) in [
            (Method::GET, "/api/v1/me/sessions"),
            (Method::GET, "/api/v1/me/grants"),
            (Method::POST, "/api/v1/me/password/change"),
            (Method::DELETE, "/api/v1/me/mfa/totp"),
            (Method::GET, "/oauth/authorize"),
            (Method::POST, "/oauth/authorize"),
            (Method::GET, "/api/v1/me/mfa/totp/enrollment"),
            (
                Method::POST,
                "/api/v1/me/passkeys/registration/verify/extra",
            ),
        ] {
            assert!(!binding_route(&method, path));
        }
    }
    #[test]
    fn reauthentication_methods_describe_actual_factors_and_errors_keep_contract_status() {
        let id = Uuid::new_v4();
        let now = OffsetDateTime::UNIX_EPOCH;
        let mut me = MeView {
            user: UserView {
                id,
                sub: id,
                email: "fixture@example.test".into(),
                email_verified: true,
                display_name: None,
                status: "active".into(),
                created_at: now,
            },
            session: SessionView {
                id,
                amr: vec!["pwd".into()],
                auth_time: now,
                strong_at: None,
                expires_at: now,
                created_at: now,
                user_agent: "test".into(),
                current: true,
            },
            security: SecurityView {
                totp_enabled: false,
                passkey_count: 1,
                recovery_codes_remaining: 0,
                is_admin: true,
                admin_binding_only: false,
            },
        };
        assert_eq!(
            serde_json::to_value(strong_methods(&me)).ok(),
            Some(json!(["passkey"]))
        );
        me.security.totp_enabled = true;
        me.security.recovery_codes_remaining = 1;
        assert_eq!(
            serde_json::to_value(strong_methods(&me)).ok(),
            Some(json!(["totp", "recovery_code", "passkey"]))
        );
        assert_eq!(
            failure(AdminError::Forbidden, id).status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            failure(AdminError::LastAdministrator, id).status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            failure(AdminError::TargetIneligible, id).status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            failure(AdminError::Unavailable, id).status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
}
