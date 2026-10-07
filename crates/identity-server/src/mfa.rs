//! Actual TOTP/recovery challenge endpoints. Purpose and user binding are database authority.
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
    routing::{delete, post},
};
use identity_core::security::{Token, token_digest};
use identity_store::{
    mfa::{
        EnrollmentConfirmInput, FactorMethod, FactorOutcome, FactorVerificationInput,
        LoginCompletion, MfaAudit, MfaError,
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
struct FactorRequest {
    challenge_id: Uuid,
    code: String,
}
impl Drop for FactorRequest {
    fn drop(&mut self) {
        self.code.zeroize();
    }
}

pub fn mfa_routes(state: AuthAppState) -> Router {
    Router::new()
        .route("/api/v1/auth/mfa/totp/verify", post(verify_totp))
        .route("/api/v1/auth/mfa/recovery/verify", post(verify_recovery))
        .route("/api/v1/me/reauth/totp", post(reauth_totp))
        .route("/api/v1/me/mfa/totp/enrollment", post(enroll))
        .route(
            "/api/v1/me/mfa/totp/enrollment/confirm",
            post(confirm_enrollment),
        )
        .route("/api/v1/me/mfa/totp", delete(disable))
        .route("/api/v1/me/mfa/recovery-codes/regenerate", post(regenerate))
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
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "DEPENDENCY_UNAVAILABLE",
                id,
            )
        })?;
    Ok((id, source))
}
fn session(state: &AuthAppState, request: &Request, id: Uuid) -> Result<Digest, ApiError> {
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
fn error(error: MfaError, id: Uuid) -> Response {
    if let MfaError::ReauthRequired { strong, methods } = error {
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
        MfaError::InvalidSession => (StatusCode::UNAUTHORIZED, "AUTH_SESSION_REQUIRED"),
        MfaError::InvalidChallenge => (StatusCode::UNAUTHORIZED, "AUTH_CHALLENGE_INVALID"),
        MfaError::ExpiredChallenge => (StatusCode::CONFLICT, "AUTH_CHALLENGE_EXPIRED"),
        MfaError::ConsumedChallenge | MfaError::AttemptsExhausted => {
            (StatusCode::CONFLICT, "AUTH_CHALLENGE_CONSUMED")
        }
        MfaError::InvalidCode => (StatusCode::UNAUTHORIZED, "AUTH_FACTOR_INVALID"),
        MfaError::AlreadyConfigured => (StatusCode::CONFLICT, "STATE_CONFLICT"),
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
        .map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "DEPENDENCY_UNAVAILABLE",
                id,
            )
        })? {
        LimitDecision::Allowed => Ok(()),
        LimitDecision::Limited { retry_after, .. } => Err(ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "RATE_LIMITED",
            request_id: id,
            retry_after: Some(retry_after),
        }),
    }
}
fn digest(token: &Token) -> Digest {
    Digest::from_bytes(token_digest(token.expose()))
}
async fn verify_totp(state: State<AuthAppState>, request: Request) -> Result<Response, ApiError> {
    verify(state.0, request, FactorMethod::Totp, false).await
}
async fn verify_recovery(
    state: State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    verify(state.0, request, FactorMethod::Recovery, false).await
}
async fn reauth_totp(state: State<AuthAppState>, request: Request) -> Result<Response, ApiError> {
    verify(state.0, request, FactorMethod::Totp, true).await
}
async fn verify(
    state: AuthAppState,
    request: Request,
    method: FactorMethod,
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
    let mut input = read_json::<FactorRequest>(request).await?;
    budget(&state, source, id, Some(input.challenge_id)).await?;
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
    let preauth = Token::generate().map_err(|_| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "DEPENDENCY_UNAVAILABLE",
            id,
        )
    })?;
    let preauth_csrf = Token::generate().map_err(|_| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "DEPENDENCY_UNAVAILABLE",
            id,
        )
    })?;
    match state.mfa.verify_factor(&FactorVerificationInput{challenge_id:input.challenge_id,code:Zeroizing::new(std::mem::take(&mut input.code)),method,reauth_only,preauth_hash:if reauth_only {None}else{preauth_hash},session_hash,login:LoginCompletion{session_id:Uuid::new_v4(),session_token_hash:digest(&token),session_csrf_hash:digest(&csrf),new_preauth_hash:digest(&preauth),new_preauth_csrf_hash:digest(&preauth_csrf),user_agent:"浏览器".into()},audit:audit(&state,source,id)}).await {
        Err(failure)=>Ok(error(failure,id)),Ok(FactorOutcome::Authenticated{user,session})=>{let mut response=Json(json!({"status":"authenticated","user":user,"session":session,"csrf_token":csrf.expose()})).into_response();response.headers_mut().append(header::SET_COOKIE,state.security.identity_cookie(token.expose()).map_err(|_|ApiError::new(StatusCode::SERVICE_UNAVAILABLE,"DEPENDENCY_UNAVAILABLE",id))?);response.headers_mut().append(header::SET_COOKIE,state.security.preauth_cookie(preauth.expose()).map_err(|_|ApiError::new(StatusCode::SERVICE_UNAVAILABLE,"DEPENDENCY_UNAVAILABLE",id))?);Ok(response)},Ok(FactorOutcome::Reauthenticated{reauthenticated_at,valid_until,amr})=>Ok(Json(json!({"status":"reauthenticated","reauthenticated_at":reauthenticated_at.format(&Rfc3339).map_err(|_|ApiError::new(StatusCode::SERVICE_UNAVAILABLE,"DEPENDENCY_UNAVAILABLE",id))?,"valid_until":valid_until.format(&Rfc3339).map_err(|_|ApiError::new(StatusCode::SERVICE_UNAVAILABLE,"DEPENDENCY_UNAVAILABLE",id))?,"strong":true,"amr":amr})).into_response())
    }
}
async fn enroll(State(state): State<AuthAppState>, request: Request) -> Result<Response, ApiError> {
    let (id, source) = context(&request)?;
    let session_hash = session(&state, &request, id)?;
    budget(&state, source, id, None).await?;
    match state.mfa.enroll(session_hash,audit(&state,source,id)).await {Err(failure)=>Ok(error(failure,id)),Ok(enrollment)=>Ok(Json(json!({"challenge_id":enrollment.challenge_id,"purpose":"totp_enrollment","secret":enrollment.secret_base32.as_str(),"otpauth_uri":enrollment.otpauth_uri.as_str(),"expires_at":enrollment.expires_at.format(&Rfc3339).map_err(|_|ApiError::new(StatusCode::SERVICE_UNAVAILABLE,"DEPENDENCY_UNAVAILABLE",id))?})).into_response())}
}
async fn confirm_enrollment(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let (id, source) = context(&request)?;
    let session_hash = session(&state, &request, id)?;
    let mut input = read_json::<FactorRequest>(request).await?;
    budget(&state, source, id, Some(input.challenge_id)).await?;
    match state.mfa.confirm_enrollment(&EnrollmentConfirmInput{session_hash,challenge_id:input.challenge_id,code:Zeroizing::new(std::mem::take(&mut input.code)),audit:audit(&state,source,id)}).await {Err(failure)=>Ok(error(failure,id)),Ok(codes)=>Ok(Json(json!({"status":"totp_enabled","recovery_codes":{"codes":codes.expose(),"shown_once":true}})).into_response())}
}
async fn disable(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let (id, source) = context(&request)?;
    let session_hash = session(&state, &request, id)?;
    budget(&state, source, id, None).await?;
    match state
        .mfa
        .disable_totp(session_hash, audit(&state, source, id))
        .await
    {
        Err(failure) => Ok(error(failure, id)),
        Ok(()) => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}
async fn regenerate(
    State(state): State<AuthAppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let (id, source) = context(&request)?;
    let session_hash = session(&state, &request, id)?;
    budget(&state, source, id, None).await?;
    match state
        .mfa
        .regenerate_recovery(session_hash, audit(&state, source, id))
        .await
    {
        Err(failure) => Ok(error(failure, id)),
        Ok(codes) => Ok(Json(json!({"codes":codes.expose(),"shown_once":true})).into_response()),
    }
}
