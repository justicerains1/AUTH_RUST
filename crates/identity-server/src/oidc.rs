//! Actual OIDC metadata, code exchange and scope-filtered userinfo. No browser Cookie authenticates clients.
use crate::{
    accounts::AuthAppState,
    security::{LimitDecision, LimitPolicy, unique_header},
};
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine, prelude::BASE64_STANDARD};
use identity_core::security::token_digest;
use identity_store::{
    repository::Digest,
    tokens::{ExchangeInput, TokenError},
};
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

pub fn oidc_routes(state: AuthAppState) -> Router {
    Router::new()
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/oauth/jwks", get(jwks))
        .route("/oauth/token", post(token))
        .route("/oauth/userinfo", get(userinfo))
        .with_state(state)
}
fn protocol_error(status: StatusCode, code: &'static str) -> Response {
    let mut response = (status, Json(json!({"error":code}))).into_response();
    if status == StatusCode::UNAUTHORIZED {
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static("Basic realm=\"identity\""),
        );
    }
    response
}
fn mapped_error(error: TokenError) -> Response {
    match error {
        TokenError::InvalidClient => protocol_error(StatusCode::UNAUTHORIZED, "invalid_client"),
        TokenError::InvalidGrant => protocol_error(StatusCode::BAD_REQUEST, "invalid_grant"),
        TokenError::InvalidToken => bearer_error(),
        TokenError::Unavailable | TokenError::SigningFailed => {
            protocol_error(StatusCode::SERVICE_UNAVAILABLE, "temporarily_unavailable")
        }
    }
}
fn bearer_error() -> Response {
    let mut response = protocol_error(StatusCode::UNAUTHORIZED, "invalid_token");
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Bearer error=\"invalid_token\""),
    );
    response
}

async fn discovery(State(state): State<AuthAppState>) -> Response {
    let issuer = &state.inner.issuer;
    let mut response=Json(json!({"issuer":issuer,"authorization_endpoint":format!("{issuer}/oauth/authorize"),"token_endpoint":format!("{issuer}/oauth/token"),"userinfo_endpoint":format!("{issuer}/oauth/userinfo"),"jwks_uri":format!("{issuer}/oauth/jwks"),"scopes_supported":["openid","profile","email"],"response_types_supported":["code"],"grant_types_supported":["authorization_code"],"subject_types_supported":["public"],"id_token_signing_alg_values_supported":["RS256"],"token_endpoint_auth_methods_supported":["client_secret_basic"],"code_challenge_methods_supported":["S256"],"claims_supported":["iss","sub","aud","exp","iat","auth_time","amr","sid","nonce","display_name","email","email_verified"],"request_parameter_supported":false,"request_uri_parameter_supported":false,"claims_parameter_supported":false})).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=300"),
    );
    response
}
async fn jwks(State(state): State<AuthAppState>) -> Response {
    let mut response = Json(state.signer.jwks()).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=300"),
    );
    response
}
fn encoding_valid(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    true
}
fn decode_basic_component(value: &str) -> Result<String, ()> {
    if !encoding_valid(value) {
        return Err(());
    }
    let bytes = value.as_bytes();
    let mut decoded = Zeroizing::new(Vec::with_capacity(bytes.len()));
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' => {
                let high = (bytes[index + 1] as char).to_digit(16).ok_or(())?;
                let low = (bytes[index + 2] as char).to_digit(16).ok_or(())?;
                decoded.push(u8::try_from((high << 4) | low).map_err(|_| ())?);
                index += 3;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded.to_vec()).map_err(|_| ())
}
fn basic_credentials(request: &Request) -> Result<(String, Zeroizing<String>), ()> {
    let header = unique_header(request.headers(), "authorization")?.ok_or(())?;
    let (scheme, payload) = header.split_once(' ').ok_or(())?;
    if !scheme.eq_ignore_ascii_case("basic") || payload.is_empty() || payload.contains(' ') {
        return Err(());
    }
    let bytes = Zeroizing::new(BASE64_STANDARD.decode(payload).map_err(|_| ())?);
    let text = std::str::from_utf8(&bytes).map_err(|_| ())?;
    let (id, secret) = text.split_once(':').ok_or(())?;
    let id = decode_basic_component(id)?;
    let secret = Zeroizing::new(decode_basic_component(secret)?);
    if id.is_empty() || id.len() > 128 || secret.is_empty() || secret.len() > 512 {
        return Err(());
    }
    Ok((id, secret))
}
fn parse_form(text: &str) -> Result<ParsedForm, ()> {
    let mut values = ParsedForm(BTreeMap::new());
    for part in text.split('&') {
        let (key, value) = part.split_once('=').unwrap_or((part, ""));
        let key = decode_basic_component(key)?;
        let value = decode_basic_component(value)?;
        if values.0.insert(key, value).is_some() {
            return Err(());
        }
    }
    Ok(values)
}
struct ParsedForm(BTreeMap<String, String>);
impl std::ops::Deref for ParsedForm {
    type Target = BTreeMap<String, String>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Drop for ParsedForm {
    fn drop(&mut self) {
        self.0.values_mut().for_each(Zeroize::zeroize);
    }
}
async fn token(State(state): State<AuthAppState>, request: Request) -> Response {
    let id = request
        .extensions()
        .get::<crate::security::RequestId>()
        .map_or_else(Uuid::new_v4, |id| id.0);
    let source = request
        .extensions()
        .get::<crate::security::TrustedSource>()
        .map(|source| source.0);
    let (client_id, secret) = match basic_credentials(&request) {
        Ok(credentials) => credentials,
        Err(_) => return protocol_error(StatusCode::UNAUTHORIZED, "invalid_client"),
    };
    let Some(source) = source else {
        return protocol_error(StatusCode::SERVICE_UNAVAILABLE, "temporarily_unavailable");
    };
    match state
        .security
        .check_limit(LimitPolicy::Token, source, Some(&client_id), None)
        .await
    {
        Err(_) => {
            return protocol_error(StatusCode::SERVICE_UNAVAILABLE, "temporarily_unavailable");
        }
        Ok(LimitDecision::Allowed) => {}
        Ok(LimitDecision::Limited { retry_after, .. }) => {
            let mut response =
                protocol_error(StatusCode::TOO_MANY_REQUESTS, "temporarily_unavailable");
            if let Ok(value) = HeaderValue::from_str(&retry_after.to_string()) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
            return response;
        }
    }
    if !unique_header(request.headers(), "content-type")
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
    let client = match state.tokens.authenticate_client(&client_id, &secret).await {
        Ok(client) => client,
        Err(error) => return mapped_error(error),
    };
    let body = match to_bytes(request.into_body(), 65536).await {
        Ok(body) => body,
        Err(_) => return protocol_error(StatusCode::BAD_REQUEST, "invalid_request"),
    };
    let body = Zeroizing::new(body.to_vec());
    let text = match std::str::from_utf8(&body) {
        Ok(text) => text,
        Err(_) => return protocol_error(StatusCode::BAD_REQUEST, "invalid_request"),
    };
    let values = match parse_form(text) {
        Ok(values) => values,
        Err(_) => return protocol_error(StatusCode::BAD_REQUEST, "invalid_request"),
    };
    if values.keys().any(|key| {
        !matches!(
            key.as_str(),
            "grant_type" | "code" | "redirect_uri" | "code_verifier"
        )
    }) {
        return protocol_error(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let Some(grant_type) = values.get("grant_type") else {
        return protocol_error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    if grant_type != "authorization_code" {
        return protocol_error(StatusCode::BAD_REQUEST, "unsupported_grant_type");
    }
    let (Some(code), Some(redirect_uri), Some(verifier)) = (
        values.get("code"),
        values.get("redirect_uri"),
        values.get("code_verifier"),
    ) else {
        return protocol_error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    if code.len() != 43 || redirect_uri.is_empty() || redirect_uri.len() > 2048 {
        return protocol_error(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let audit = identity_store::oauth::OAuthAudit {
        request_id: id,
        source_hash: state.security.source_digest(source),
    };
    match state.tokens.exchange_authorization_code(&ExchangeInput{client,code_hash:Digest::from_bytes(token_digest(code)),redirect_uri:redirect_uri.clone(),code_verifier:Zeroizing::new(verifier.clone()),request_id:audit.request_id,source_hash:audit.source_hash},&state.signer).await{Err(error)=>mapped_error(error),Ok(bundle)=>Json(json!({"access_token":bundle.access_token.expose(),"refresh_token":bundle.refresh_token.expose(),"id_token":bundle.id_token.as_str(),"token_type":"Bearer","expires_in":bundle.expires_in,"scope":bundle.scope})).into_response()}
}
async fn userinfo(State(state): State<AuthAppState>, request: Request) -> Response {
    let header = match unique_header(request.headers(), "authorization") {
        Ok(Some(value)) => value,
        _ => return bearer_error(),
    };
    let Some((scheme, token)) = header.split_once(' ') else {
        return bearer_error();
    };
    if !scheme.eq_ignore_ascii_case("bearer")
        || token.is_empty()
        || token.contains(' ')
        || token.len() > 512
    {
        return bearer_error();
    }
    match state
        .tokens
        .userinfo(Digest::from_bytes(token_digest(token)))
        .await
    {
        Ok(info) => Json(info).into_response(),
        Err(error) => mapped_error(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basic_form_encoding_decodes_once_and_preserves_colon_plus() {
        assert_eq!(
            decode_basic_component("client%3Aid"),
            Ok("client:id".into())
        );
        assert_eq!(
            decode_basic_component("secret%2Bvalue"),
            Ok("secret+value".into())
        );
        assert_eq!(
            decode_basic_component("percent%2520literal"),
            Ok("percent%20literal".into())
        );
        assert!(decode_basic_component("invalid%GG").is_err());
        assert!(decode_basic_component("invalid%FF").is_err());
    }
    #[test]
    fn duplicate_form_credentials_or_grant_parameters_are_not_selected() {
        assert!(parse_form("code=first&code=second").is_err());
        assert!(parse_form("grant_type=authorization_code&code_verifier=example%GG").is_err());
    }
}
