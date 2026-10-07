//! Strict OAuth authorization inputs. These value objects do not issue grants or tokens.
use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fmt};
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    OpenId,
    Profile,
    Email,
}
impl Scope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenId => "openid",
            Self::Profile => "profile",
            Self::Email => "email",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prompt {
    Login,
    Consent,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OAuthInputError {
    InvalidRequest,
    InvalidScope,
    UnsupportedResponseType,
}
impl fmt::Display for OAuthInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidRequest => "invalid_request",
            Self::InvalidScope => "invalid_scope",
            Self::UnsupportedResponseType => "unsupported_response_type",
        })
    }
}
impl std::error::Error for OAuthInputError {}

pub struct AuthorizationRequest {
    pub client_id: String,
    pub redirect_uri: String,
    pub scopes: Vec<Scope>,
    pub state: String,
    pub nonce: String,
    pub code_challenge: String,
    pub prompt: Vec<Prompt>,
    pub max_age: Option<u64>,
}
impl fmt::Debug for AuthorizationRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthorizationRequest")
            .field("scopes", &self.scopes)
            .field("prompt", &self.prompt)
            .field("private_parameters", &"[REDACTED]")
            .finish()
    }
}

fn percent_encoding_valid(text: &str) -> bool {
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

impl AuthorizationRequest {
    /// Parse GET query or POST form identically; duplicate critical inputs are never selected silently.
    pub fn parse(form: &str) -> Result<Self, OAuthInputError> {
        if form.len() > 65_536 || !percent_encoding_valid(form) {
            return Err(OAuthInputError::InvalidRequest);
        }
        let mut values = BTreeMap::new();
        for (name, value) in url::form_urlencoded::parse(form.as_bytes()) {
            if values
                .insert(name.into_owned(), value.into_owned())
                .is_some()
            {
                return Err(OAuthInputError::InvalidRequest);
            }
        }
        let required = |key| {
            values
                .get(key)
                .filter(|value| !value.is_empty())
                .cloned()
                .ok_or(OAuthInputError::InvalidRequest)
        };
        if required("response_type")? != "code" {
            return Err(OAuthInputError::UnsupportedResponseType);
        }
        let client_id = required("client_id")?;
        if client_id.len() > 128
            || !client_id.is_ascii()
            || client_id.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(OAuthInputError::InvalidRequest);
        }
        let redirect_uri = required("redirect_uri")?;
        if redirect_uri.len() > 2048 {
            return Err(OAuthInputError::InvalidRequest);
        }
        let scopes = parse_scopes(&required("scope")?)?;
        let state = required("state")?;
        let nonce = required("nonce")?;
        if !valid_state(&state) || !valid_state(&nonce) {
            return Err(OAuthInputError::InvalidRequest);
        }
        let code_challenge = required("code_challenge")?;
        if required("code_challenge_method")? != "S256" || !valid_challenge(&code_challenge) {
            return Err(OAuthInputError::InvalidRequest);
        }
        let prompt = match values.get("prompt") {
            None => Vec::new(),
            Some(value) => {
                let mut parsed = Vec::new();
                for piece in value.split(' ') {
                    let item = match piece {
                        "login" => Prompt::Login,
                        "consent" => Prompt::Consent,
                        "none" => Prompt::None,
                        _ => return Err(OAuthInputError::InvalidRequest),
                    };
                    if parsed.contains(&item) {
                        return Err(OAuthInputError::InvalidRequest);
                    }
                    parsed.push(item);
                }
                if parsed.contains(&Prompt::None) && parsed.len() != 1 {
                    return Err(OAuthInputError::InvalidRequest);
                }
                parsed
            }
        };
        let max_age = values
            .get("max_age")
            .map(|text| {
                if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(OAuthInputError::InvalidRequest);
                }
                text.parse().map_err(|_| OAuthInputError::InvalidRequest)
            })
            .transpose()?;
        Ok(Self {
            client_id,
            redirect_uri,
            scopes,
            state,
            nonce,
            code_challenge,
            prompt,
            max_age,
        })
    }
}

pub fn parse_scopes(text: &str) -> Result<Vec<Scope>, OAuthInputError> {
    let mut scopes = Vec::new();
    for piece in text.split(' ') {
        let scope = match piece {
            "openid" => Scope::OpenId,
            "profile" => Scope::Profile,
            "email" => Scope::Email,
            _ => return Err(OAuthInputError::InvalidScope),
        };
        if scopes.contains(&scope) {
            return Err(OAuthInputError::InvalidScope);
        }
        scopes.push(scope);
    }
    if !scopes.contains(&Scope::OpenId) {
        return Err(OAuthInputError::InvalidScope);
    }
    scopes.sort();
    Ok(scopes)
}
pub fn valid_state(text: &str) -> bool {
    (16..=512).contains(&text.len()) && text.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
}
pub fn valid_challenge(text: &str) -> bool {
    text.len() == 43
        && BASE64_URL_SAFE_NO_PAD
            .decode(text)
            .is_ok_and(|bytes| bytes.len() == 32)
}
pub fn pkce_s256(verifier: &str) -> Result<String, OAuthInputError> {
    if !(43..=128).contains(&verifier.len())
        || !verifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._~".contains(&byte))
    {
        return Err(OAuthInputError::InvalidRequest);
    }
    Ok(BASE64_URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())))
}

/// Validate registrations; authorization later compares the original stored string exactly.
pub fn validate_redirect_uri(text: &str, production: bool) -> Result<(), OAuthInputError> {
    if text.is_empty()
        || text.len() > 2048
        || text.contains('*')
        || text
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        || !percent_encoding_valid(text)
    {
        return Err(OAuthInputError::InvalidRequest);
    }
    let uri = Url::parse(text).map_err(|_| OAuthInputError::InvalidRequest)?;
    if !uri.username().is_empty()
        || uri.password().is_some()
        || uri.fragment().is_some()
        || uri.host_str().is_none()
    {
        return Err(OAuthInputError::InvalidRequest);
    }
    let allowed = uri.scheme() == "https"
        || (!production
            && uri.scheme() == "http"
            && matches!(uri.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")));
    if !allowed {
        return Err(OAuthInputError::InvalidRequest);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn valid_request() -> String {
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs([
                ("client_id", "demo-a"),
                ("response_type", "code"),
                ("redirect_uri", "https://app.example/callback"),
                ("scope", "openid email"),
                ("state", "state_example_123456"),
                ("nonce", "nonce_example_123456"),
                (
                    "code_challenge",
                    "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
                ),
                ("code_challenge_method", "S256"),
            ])
            .finish()
    }
    #[test]
    fn valid_authorization_is_typed_and_secret_debug_is_redacted() -> Result<(), OAuthInputError> {
        let request = AuthorizationRequest::parse(&valid_request())?;
        assert_eq!(request.scopes, vec![Scope::OpenId, Scope::Email]);
        assert!(!format!("{request:?}").contains("state_example"));
        Ok(())
    }
    #[test]
    fn rejects_duplicate_parameters_weak_pkce_and_invalid_prompt() {
        for extra in [
            "&client_id=other",
            "&state=other",
            "&prompt=none+login",
            "&prompt=login+login",
            "&max_age=-1",
            "&max_age=18446744073709551616",
        ] {
            assert!(AuthorizationRequest::parse(&(valid_request() + extra)).is_err());
        }
        assert!(AuthorizationRequest::parse(&valid_request().replace("S256", "plain")).is_err());
        assert!(
            AuthorizationRequest::parse(&valid_request().replace("openid+email", "email")).is_err()
        );
        assert!(AuthorizationRequest::parse(&(valid_request() + "&bad=%GG")).is_err());
    }
    #[test]
    fn pkce_matches_rfc7636_and_enforces_verifier_charset() -> Result<(), OAuthInputError> {
        assert_eq!(
            pkce_s256("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")?,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert!(pkce_s256(&"a".repeat(42)).is_err());
        assert!(pkce_s256(&"a".repeat(129)).is_err());
        assert!(pkce_s256(&"/".repeat(43)).is_err());
        assert!(!valid_challenge(&"Z".repeat(43)));
        Ok(())
    }
    #[test]
    fn callbacks_require_exact_safe_registration_and_loopback_http_only() {
        assert!(validate_redirect_uri("https://app.example/callback?fixed=1", true).is_ok());
        assert!(validate_redirect_uri("http://localhost:3000/callback", false).is_ok());
        for text in [
            "http://app.example/callback",
            "https://app.example/*",
            "https://user:pass@app.example/callback",
            "https://app.example/callback#token",
            " https://app.example/callback",
        ] {
            assert!(validate_redirect_uri(text, true).is_err());
        }
        assert!(validate_redirect_uri("http://remote.example/callback", false).is_err());
    }
}
