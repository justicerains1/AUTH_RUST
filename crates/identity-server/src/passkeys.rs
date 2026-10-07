//! WebAuthn HTTP DTOs and browser ceremony routes. The library and database own all trust decisions.
use crate::{
    accounts::AuthAppState,
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
    routing::{get, post},
};
use identity_core::security::{Token, token_digest};
use identity_store::{
    mfa::{FactorOutcome, LoginCompletion, MfaAudit},
    passkeys::{PasskeyAssertionInput, PasskeyStoreError, RegistrationInput},
    repository::Digest,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientExtensions {
    #[serde(rename = "credProps", skip_serializing_if = "Option::is_none")]
    cred_props: Option<CredProps>,
    #[serde(skip_serializing_if = "Option::is_none")]
    appid: Option<bool>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CredProps {
    rk: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AttestationResponse {
    #[serde(rename = "clientDataJSON")]
    client_data_json: String,
    attestation_object: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    transports: Option<Vec<Transport>>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Transport {
    Usb,
    Nfc,
    Ble,
    Internal,
    Hybrid,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum CredentialType {
    #[serde(rename = "public-key")]
    PublicKey,
}
#[derive(Serialize, Deserialize)]
enum Attachment {
    #[serde(rename = "platform")]
    Platform,
    #[serde(rename = "cross-platform")]
    CrossPlatform,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Attestation {
    id: String,
    raw_id: String,
    #[serde(rename = "type")]
    kind: CredentialType,
    response: AttestationResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    authenticator_attachment: Option<Attachment>,
    client_extension_results: ClientExtensions,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AssertionResponse {
    #[serde(rename = "clientDataJSON")]
    client_data_json: String,
    authenticator_data: String,
    signature: String,
    user_handle: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Assertion {
    id: String,
    raw_id: String,
    #[serde(rename = "type")]
    kind: CredentialType,
    response: AssertionResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    authenticator_attachment: Option<Attachment>,
    client_extension_results: ClientExtensions,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterRequest {
    challenge_id: Uuid,
    name: String,
    credential: Attestation,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifyRequest {
    challenge_id: Uuid,
    credential: Assertion,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NameRequest {
    name: String,
}

pub fn passkey_routes(state: AuthAppState) -> Router {
    Router::new()
        .route(
            "/api/v1/me/passkeys/registration/options",
            post(registration_options),
        )
        .route(
            "/api/v1/me/passkeys/registration/verify",
            post(registration_verify),
        )
        .route("/api/v1/auth/passkeys/options", post(login_options))
        .route("/api/v1/auth/passkeys/verify", post(login_verify))
        .route("/api/v1/me/reauth/passkeys/options", post(reauth_options))
        .route("/api/v1/me/reauth/passkeys/verify", post(reauth_verify))
        .route("/api/v1/me/passkeys", get(list))
        .route(
            "/api/v1/me/passkeys/{id}",
            get(detail).patch(rename).delete(remove),
        )
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
fn identity(state: &AuthAppState, request: &Request, id: Uuid) -> Result<Digest, ApiError> {
    state
        .security
        .identity_digest(request.headers())
        .map_err(|_| ApiError::new(StatusCode::UNAUTHORIZED, "AUTH_SESSION_REQUIRED", id))?
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "AUTH_SESSION_REQUIRED", id))
}
fn audit(state: &AuthAppState, source: std::net::IpAddr, id: Uuid) -> MfaAudit {
    MfaAudit {
        request_id: id,
        source_hash: state.security.source_digest(source),
    }
}
fn error(failure: PasskeyStoreError, id: Uuid) -> Response {
    if let PasskeyStoreError::ReauthRequired { strong, methods } = failure {
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
    let (status, code) = match failure {
        PasskeyStoreError::InvalidSession => (StatusCode::UNAUTHORIZED, "AUTH_SESSION_REQUIRED"),
        PasskeyStoreError::InvalidChallenge => (StatusCode::UNAUTHORIZED, "AUTH_CHALLENGE_INVALID"),
        PasskeyStoreError::ExpiredChallenge => (StatusCode::CONFLICT, "AUTH_CHALLENGE_EXPIRED"),
        PasskeyStoreError::ConsumedChallenge => (StatusCode::CONFLICT, "AUTH_CHALLENGE_CONSUMED"),
        PasskeyStoreError::InvalidCredential => (StatusCode::UNAUTHORIZED, "AUTH_FACTOR_INVALID"),
        PasskeyStoreError::NotFound => (StatusCode::NOT_FOUND, "RESOURCE_NOT_FOUND"),
        PasskeyStoreError::LimitReached => (StatusCode::CONFLICT, "AUTH_FACTOR_LIMIT_REACHED"),
        PasskeyStoreError::InvalidName => (StatusCode::UNPROCESSABLE_ENTITY, "INPUT_INVALID"),
        _ => (StatusCode::SERVICE_UNAVAILABLE, "DEPENDENCY_UNAVAILABLE"),
    };
    ApiError::new(status, code, id).into_response()
}
async fn budget(
    state: &AuthAppState,
    source: std::net::IpAddr,
    id: Uuid,
    challenge: Option<Uuid>,
) -> Result<(), ApiError> {
    match state
        .security
        .check_limit(LimitPolicy::Challenge, source, None, challenge)
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
fn valid_binary(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 87382
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
}
fn validate_attestation(value: &Attestation) -> bool {
    value.id == value.raw_id
        && valid_binary(&value.id)
        && valid_binary(&value.response.client_data_json)
        && valid_binary(&value.response.attestation_object)
}
fn validate_assertion(value: &Assertion) -> bool {
    value.id == value.raw_id
        && valid_binary(&value.id)
        && valid_binary(&value.response.client_data_json)
        && valid_binary(&value.response.authenticator_data)
        && valid_binary(&value.response.signature)
        && value
            .response
            .user_handle
            .as_ref()
            .is_none_or(|value| valid_binary(value) && value.len() <= 86)
}
async fn registration_options(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let (id, source) = context(&request)?;
    let hash = identity(&state, &request, id)?;
    budget(&state, source, id, None).await?;
    match state.passkeys.begin_registration(hash,audit(&state,source,id)).await{Err(failure)=>Ok(error(failure,id)),Ok(options)=>Ok(Json(json!({"challenge_id":options.challenge_id,"purpose":"passkey_registration","expires_at":options.expires_at.format(&Rfc3339).map_err(|_|unavailable(id))?,"publicKey":options.public_key})).into_response())}
}
async fn registration_verify(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let (id, source) = context(&request)?;
    let hash = identity(&state, &request, id)?;
    let input = read_json::<RegisterRequest>(request).await?;
    if !validate_attestation(&input.credential) {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "INPUT_INVALID",
            id,
        ));
    }
    budget(&state, source, id, Some(input.challenge_id)).await?;
    match state
        .passkeys
        .finish_registration(&RegistrationInput {
            session_hash: hash,
            challenge_id: input.challenge_id,
            name: input.name,
            credential: serde_json::to_value(input.credential).map_err(|_| unavailable(id))?,
            audit: audit(&state, source, id),
        })
        .await
    {
        Err(failure) => Ok(error(failure, id)),
        Ok(view) => Ok((StatusCode::CREATED, Json(view)).into_response()),
    }
}
async fn login_options(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let (id, source) = context(&request)?;
    let preauth = state
        .security
        .preauth_digest(request.headers())
        .map_err(|_| ApiError::new(StatusCode::UNAUTHORIZED, "AUTH_CHALLENGE_INVALID", id))?
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "AUTH_CHALLENGE_INVALID", id))?;
    budget(&state, source, id, None).await?;
    match state.passkeys.begin_login(preauth).await{Err(failure)=>Ok(error(failure,id)),Ok(options)=>Ok(Json(json!({"challenge_id":options.challenge_id,"purpose":"passkey_login","expires_at":options.expires_at.format(&Rfc3339).map_err(|_|unavailable(id))?,"publicKey":options.public_key})).into_response())}
}
async fn reauth_options(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let (id, source) = context(&request)?;
    let hash = identity(&state, &request, id)?;
    budget(&state, source, id, None).await?;
    match state.passkeys.begin_reauthentication(hash).await{Err(failure)=>Ok(error(failure,id)),Ok(options)=>Ok(Json(json!({"challenge_id":options.challenge_id,"purpose":"passkey_reauthentication","expires_at":options.expires_at.format(&Rfc3339).map_err(|_|unavailable(id))?,"publicKey":options.public_key})).into_response())}
}
async fn login_verify(state: State<AuthAppState>, request: Request) -> Result<Response, ApiError> {
    verify(state.0, request, false).await
}
async fn reauth_verify(state: State<AuthAppState>, request: Request) -> Result<Response, ApiError> {
    verify(state.0, request, true).await
}
fn digest(token: &Token) -> Digest {
    Digest::from_bytes(token_digest(token.expose()))
}
async fn verify(
    state: AuthAppState,
    request: Request,
    reauth_only: bool,
) -> Result<Response, ApiError> {
    let (id, source) = context(&request)?;
    let session_hash = state
        .security
        .identity_digest(request.headers())
        .map_err(|_| ApiError::new(StatusCode::UNAUTHORIZED, "AUTH_SESSION_REQUIRED", id))?;
    if reauth_only && session_hash.is_none() {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "AUTH_SESSION_REQUIRED",
            id,
        ));
    }
    let preauth_hash = state
        .security
        .preauth_digest(request.headers())
        .map_err(|_| ApiError::new(StatusCode::UNAUTHORIZED, "AUTH_CHALLENGE_INVALID", id))?;
    let input = read_json::<VerifyRequest>(request).await?;
    if !validate_assertion(&input.credential) {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "INPUT_INVALID",
            id,
        ));
    }
    budget(&state, source, id, Some(input.challenge_id)).await?;
    let token = Token::generate().map_err(|_| unavailable(id))?;
    let csrf = Token::generate().map_err(|_| unavailable(id))?;
    let preauth = Token::generate().map_err(|_| unavailable(id))?;
    let preauth_csrf = Token::generate().map_err(|_| unavailable(id))?;
    match state.passkeys.finish_login(&PasskeyAssertionInput{challenge_id:input.challenge_id,credential:serde_json::to_value(input.credential).map_err(|_|unavailable(id))?,preauth_hash:if reauth_only{None}else{preauth_hash},session_hash,login:LoginCompletion{session_id:Uuid::new_v4(),session_token_hash:digest(&token),session_csrf_hash:digest(&csrf),new_preauth_hash:digest(&preauth),new_preauth_csrf_hash:digest(&preauth_csrf),user_agent:"浏览器".into()},audit:audit(&state,source,id),reauth_only}).await{Err(failure)=>Ok(error(failure,id)),Ok(FactorOutcome::Authenticated{user,session})=>{let mut response=Json(json!({"status":"authenticated","user":user,"session":session,"csrf_token":csrf.expose()})).into_response();response.headers_mut().append(header::SET_COOKIE,state.security.identity_cookie(token.expose()).map_err(|_|unavailable(id))?);response.headers_mut().append(header::SET_COOKIE,state.security.preauth_cookie(preauth.expose()).map_err(|_|unavailable(id))?);Ok(response)},Ok(FactorOutcome::Reauthenticated{reauthenticated_at,valid_until,amr})=>Ok(Json(json!({"status":"reauthenticated","reauthenticated_at":reauthenticated_at.format(&Rfc3339).map_err(|_|unavailable(id))?,"valid_until":valid_until.format(&Rfc3339).map_err(|_|unavailable(id))?,"strong":true,"amr":amr})).into_response())}
}
fn target(request: &Request, id: Uuid) -> Result<Uuid, ApiError> {
    request
        .uri()
        .path()
        .rsplit('/')
        .next()
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "INPUT_INVALID", id))
}
async fn list(State(state): State<AuthAppState>, request: Request) -> Result<Response, ApiError> {
    let (id, _) = context(&request)?;
    let hash = identity(&state, &request, id)?;
    let mut limit = 20;
    let mut cursor = None;
    let mut limit_seen = false;
    for (key, value) in request
        .uri()
        .query()
        .map(|query| url::form_urlencoded::parse(query.as_bytes()))
        .into_iter()
        .flatten()
    {
        match key.as_ref() {
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
    match state
        .passkeys
        .page(hash, limit, cursor.as_deref(), &state.security.cursor_key())
        .await
    {
        Err(failure) => Ok(error(failure, id)),
        Ok(page) => Ok(Json(page).into_response()),
    }
}
async fn detail(State(state): State<AuthAppState>, request: Request) -> Result<Response, ApiError> {
    let (id, _) = context(&request)?;
    let hash = identity(&state, &request, id)?;
    match state.passkeys.get(hash, target(&request, id)?).await {
        Err(failure) => Ok(error(failure, id)),
        Ok(view) => Ok(Json(view).into_response()),
    }
}
async fn rename(State(state): State<AuthAppState>, request: Request) -> Result<Response, ApiError> {
    let (id, source) = context(&request)?;
    let hash = identity(&state, &request, id)?;
    let target = target(&request, id)?;
    let input = read_json::<NameRequest>(request).await?;
    match state
        .passkeys
        .rename(hash, target, &input.name, audit(&state, source, id))
        .await
    {
        Err(failure) => Ok(error(failure, id)),
        Ok(view) => Ok(Json(view).into_response()),
    }
}
async fn remove(State(state): State<AuthAppState>, request: Request) -> Result<Response, ApiError> {
    let (id, source) = context(&request)?;
    let hash = identity(&state, &request, id)?;
    match state
        .passkeys
        .delete(hash, target(&request, id)?, audit(&state, source, id))
        .await
    {
        Err(failure) => Ok(error(failure, id)),
        Ok(()) => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard_client_data_json_spelling_survives_strict_dto()
    -> Result<(), Box<dyn std::error::Error>> {
        let value = serde_json::json!({"challenge_id":Uuid::new_v4(),"name":"测试Passkey","credential":{"id":"AQID","rawId":"AQID","type":"public-key","response":{"clientDataJSON":"BAUG","attestationObject":"BwgJ","transports":["internal"]},"clientExtensionResults":{"credProps":{"rk":true}}}});
        let parsed: RegisterRequest = serde_json::from_value(value)?;
        let serialized = serde_json::to_value(parsed.credential)?;
        assert_eq!(serialized["response"]["clientDataJSON"], "BAUG");
        assert!(serialized["response"].get("clientDataJson").is_none());
        Ok(())
    }
}
