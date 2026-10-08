//! Maintained OIDC library handles wire protocol and JWT verification; this module pins the trust profile.
use crate::{config::BffConfig, store::BffTokens};
use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use identity_core::security::Token;
use openid::{Claims, Client, Discovered, Options, Pkce, PkceSha256};
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fmt, sync::Arc};
use time::{Duration, OffsetDateTime};
use url::Url;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

#[derive(Debug)]
pub enum OidcError {
    Unavailable,
    InvalidResponse,
    InvalidGrant,
}
impl fmt::Display for OidcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "identity provider unavailable",
            Self::InvalidResponse => "invalid identity provider response",
            Self::InvalidGrant => "identity grant invalid",
        })
    }
}
impl std::error::Error for OidcError {}
#[derive(Clone, Serialize, Deserialize)]
struct IdentityClaims {
    sid: Uuid,
    #[serde(flatten)]
    standard: openid::StandardClaims,
}
impl openid::CustomClaims for IdentityClaims {
    fn standard_claims(&self) -> &openid::StandardClaims {
        &self.standard
    }
}
impl openid::CompactJson for IdentityClaims {}
struct SecretBearer(openid::Bearer);
impl AsRef<openid::Bearer> for SecretBearer {
    fn as_ref(&self) -> &openid::Bearer {
        &self.0
    }
}
impl Drop for SecretBearer {
    fn drop(&mut self) {
        self.0.access_token.zeroize();
        if let Some(value) = &mut self.0.refresh_token {
            value.zeroize();
        }
        if let Some(value) = &mut self.0.id_token {
            value.zeroize();
        }
        if let Some(value) = &mut self.0.state {
            value.zeroize();
        }
    }
}
type IdentityClient = Client<Discovered, IdentityClaims>;
type AuthorizationStart = (Url, Zeroizing<String>, Zeroizing<String>, Zeroizing<String>);
#[derive(Clone)]
pub struct Oidc {
    client: Arc<IdentityClient>,
    http: reqwest::Client,
    issuer: Url,
    introspection: Url,
    jwks_uri: Url,
}
impl Oidc {
    pub async fn discover(config: &BffConfig) -> Result<Self, OidcError> {
        let mut builder = reqwest::Client::builder()
            .redirect(Policy::none())
            .connect_timeout(std::time::Duration::from_secs(3))
            .timeout(std::time::Duration::from_secs(10));
        if config.issuer_connect_host.is_some() {
            let port = config
                .issuer
                .port_or_known_default()
                .ok_or(OidcError::InvalidResponse)?;
            let addresses = tokio::net::lookup_host(("identity-web", port))
                .await
                .map_err(|_| OidcError::Unavailable)?
                .collect::<Vec<_>>();
            builder = builder.resolve_to_addrs(
                config.issuer.host_str().ok_or(OidcError::InvalidResponse)?,
                &addresses,
            );
        }
        let http = builder.build().map_err(|_| OidcError::Unavailable)?;
        let metadata_raw: serde_json::Value = http
            .get(format!(
                "{}/.well-known/openid-configuration",
                config.issuer.origin().ascii_serialization()
            ))
            .send()
            .await
            .map_err(|_| OidcError::Unavailable)?
            .error_for_status()
            .map_err(|_| OidcError::Unavailable)?
            .json()
            .await
            .map_err(|_| OidcError::InvalidResponse)?;
        let metadata: openid::Config =
            serde_json::from_value(metadata_raw.clone()).map_err(|_| OidcError::InvalidResponse)?;
        for name in [
            "introspection_endpoint",
            "revocation_endpoint",
            "end_session_endpoint",
        ] {
            let endpoint = metadata_raw
                .get(name)
                .and_then(serde_json::Value::as_str)
                .ok_or(OidcError::InvalidResponse)?;
            fixed_endpoint(
                &Url::parse(endpoint).map_err(|_| OidcError::InvalidResponse)?,
                &config.issuer,
            )?;
        }
        if metadata.issuer != config.issuer {
            return Err(OidcError::InvalidResponse);
        }
        for endpoint in [
            &metadata.authorization_endpoint,
            &metadata.token_endpoint,
            &metadata.jwks_uri,
        ] {
            fixed_endpoint(endpoint, &config.issuer)?;
        }
        if let Some(endpoint) = &metadata.userinfo_endpoint {
            fixed_endpoint(endpoint, &config.issuer)?;
        }
        let public: serde_json::Value = http
            .get(metadata.jwks_uri.clone())
            .send()
            .await
            .map_err(|_| OidcError::Unavailable)?
            .error_for_status()
            .map_err(|_| OidcError::Unavailable)?
            .json()
            .await
            .map_err(|_| OidcError::InvalidResponse)?;
        validate_public_keys(&public)?;
        let jwks = serde_json::from_value(public).map_err(|_| OidcError::InvalidResponse)?;
        let jwks_uri = metadata.jwks_uri.clone();
        let client: IdentityClient = Client::new(
            Discovered::from(metadata),
            config.client_id.clone(),
            Some(config.client_secret.to_string()),
            Some(config.callback()),
            http.clone(),
            Some(jwks),
        );
        let introspection = Url::parse(&format!(
            "{}/oauth/introspect",
            config.issuer.origin().ascii_serialization()
        ))
        .map_err(|_| OidcError::InvalidResponse)?;
        Ok(Self {
            client: Arc::new(client),
            http,
            issuer: config.issuer.clone(),
            introspection,
            jwks_uri,
        })
    }
    pub fn authorization(&self) -> Result<AuthorizationStart, OidcError> {
        let state = Token::generate().map_err(|_| OidcError::Unavailable)?;
        let nonce = Token::generate().map_err(|_| OidcError::Unavailable)?;
        let verifier = Token::generate().map_err(|_| OidcError::Unavailable)?;
        let options = Options {
            scope: Some("openid profile email".into()),
            state: Some(state.expose().into()),
            nonce: Some(nonce.expose().into()),
            ..Options::default()
        };
        let mut client = (*self.client).clone();
        client.pkce = Some(Pkce::S256(PkceSha256::replicate(verifier.expose().into())));
        let url = client.auth_url(&options);
        clear_client(&mut client);
        Ok((
            url,
            Zeroizing::new(state.expose().into()),
            Zeroizing::new(nonce.expose().into()),
            Zeroizing::new(verifier.expose().into()),
        ))
    }
    pub async fn exchange(
        &self,
        code: &str,
        verifier: &str,
        nonce: &str,
    ) -> Result<(Uuid, BffTokens), OidcError> {
        let bearer = self
            .client
            .request_token_pkce(code, Some(verifier))
            .await
            .map_err(client_error)?;
        self.validate_with_rotation(bearer, Some(nonce), None).await
    }
    pub async fn refresh(&self, tokens: &BffTokens) -> Result<(Uuid, BffTokens), OidcError> {
        // This payload came from an authenticated AEAD BFF session and was verified at insertion.
        // Do not reverify the historical signature: its public key may have been retired since login.
        let previous = session_claims(&tokens.id_token)?;
        let old = SecretBearer(openid::Bearer {
            access_token: tokens.access_token.clone(),
            token_type: "Bearer".into(),
            refresh_token: Some(tokens.refresh_token.clone()),
            id_token: Some(tokens.id_token.clone()),
            scope: None,
            state: None,
            expires_in: None,
            extra: None,
        });
        let bearer = self
            .client
            .refresh_token(&old, None)
            .await
            .map_err(client_error)?;
        let (user, fresh) = self
            .validate_with_rotation(bearer, None, Some(tokens.session_expires_at))
            .await?;
        let current = session_claims(&fresh.id_token)?; // New token has just passed SDK signature/claims validation.
        if previous.sid != current.sid
            || previous.sub() != current.sub()
            || previous.auth_time() != current.auth_time()
            || previous.amr() != current.amr()
            || fresh.refresh_token == tokens.refresh_token
            || fresh.id_token == tokens.id_token
        {
            return Err(OidcError::InvalidResponse);
        }
        Ok((user, fresh))
    }
    async fn validate_with_rotation(
        &self,
        bearer: openid::Bearer,
        nonce: Option<&str>,
        absolute: Option<OffsetDateTime>,
    ) -> Result<(Uuid, BffTokens), OidcError> {
        let bearer = SecretBearer(bearer);
        match self.validate_bundle(&bearer.0, nonce, absolute) {
            Ok(valid) => return Ok(valid),
            Err(OidcError::Unavailable) => return Err(OidcError::Unavailable),
            Err(OidcError::InvalidGrant) => return Err(OidcError::InvalidGrant),
            Err(OidcError::InvalidResponse) => {}
        }
        // Refresh only fixed public keys after receiving tokens; never resend a consumed code/refresh.
        let raw: serde_json::Value = self
            .http
            .get(self.jwks_uri.clone())
            .send()
            .await
            .map_err(|_| OidcError::Unavailable)?
            .error_for_status()
            .map_err(|_| OidcError::Unavailable)?
            .json()
            .await
            .map_err(|_| OidcError::InvalidResponse)?;
        validate_public_keys(&raw)?;
        let mut updated = self.clone();
        let mut client = (*self.client).clone();
        client.jwks = Some(serde_json::from_value(raw).map_err(|_| OidcError::InvalidResponse)?);
        updated.client = Arc::new(client);
        updated.validate_bundle(&bearer.0, nonce, absolute)
    }
    fn validate_bundle(
        &self,
        bearer: &openid::Bearer,
        nonce: Option<&str>,
        absolute: Option<OffsetDateTime>,
    ) -> Result<(Uuid, BffTokens), OidcError> {
        let now = OffsetDateTime::now_utc();
        let expires = bearer
            .expires_in
            .filter(|value| *value >= 1 && *value <= 300)
            .ok_or(OidcError::InvalidResponse)?;
        let raw_id = bearer.id_token.as_ref().ok_or(OidcError::InvalidResponse)?;
        let refresh = bearer
            .refresh_token
            .as_ref()
            .ok_or(OidcError::InvalidResponse)?;
        if !bearer.token_type.eq_ignore_ascii_case("bearer")
            || !opaque_token(&bearer.access_token)
            || !opaque_token(refresh)
            || raw_id.len() > 16384
        {
            return Err(OidcError::InvalidResponse);
        }
        let mut jwt = openid::Jws::<IdentityClaims, openid::Empty>::new_encoded(raw_id);
        let id_token = &mut jwt;
        // The SDK accepts the sole key even when a token carries a different kid. Pin it explicitly.
        let header = id_token
            .unverified_header()
            .map_err(|_| OidcError::InvalidResponse)?;
        let kid = header.registered.key_id.ok_or(OidcError::InvalidResponse)?;
        if self
            .client
            .jwks
            .as_ref()
            .is_none_or(|keys| keys.find(&kid).is_none())
        {
            return Err(OidcError::InvalidResponse);
        }
        self.client
            .decode_token(id_token)
            .map_err(|_| OidcError::InvalidResponse)?;
        self.client
            .validate_token(id_token, nonce, None)
            .map_err(|_| OidcError::InvalidResponse)?;
        let claims = id_token.payload().map_err(|_| OidcError::InvalidResponse)?;
        if claims.iat() > now.unix_timestamp().saturating_add(120)
            || claims.iat() < 0
            || claims
                .exp()
                .checked_sub(claims.iat())
                .is_none_or(|age| age <= 0 || age > 300)
        {
            return Err(OidcError::InvalidResponse);
        }
        let auth = claims
            .auth_time()
            .filter(|auth| *auth >= 0 && *auth <= claims.iat())
            .ok_or(OidcError::InvalidResponse)?;
        if claims.sid.is_nil()
            || claims.amr().is_none_or(|values| {
                values.is_empty()
                    || values.len() > 8
                    || values.iter().any(|value| {
                        !matches!(value.as_str(), "pwd" | "otp" | "rcv" | "user" | "hwk")
                    })
            })
            || nonce.is_none() && claims.nonce().is_some()
        {
            return Err(OidcError::InvalidResponse);
        }
        let user = Uuid::parse_str(claims.sub()).map_err(|_| OidcError::InvalidResponse)?;
        let authenticated_at =
            OffsetDateTime::from_unix_timestamp(auth).map_err(|_| OidcError::InvalidResponse)?;
        let derived_absolute = authenticated_at
            .checked_add(Duration::hours(12))
            .ok_or(OidcError::InvalidResponse)?;
        let session_exp = absolute.unwrap_or(derived_absolute).min(derived_absolute);
        if session_exp <= now {
            return Err(OidcError::InvalidResponse);
        }
        Ok((
            user,
            BffTokens {
                access_token: bearer.access_token.clone(),
                refresh_token: refresh.clone(),
                id_token: raw_id.clone(),
                access_expires_at: (now
                    + Duration::seconds(
                        i64::try_from(expires).map_err(|_| OidcError::InvalidResponse)?,
                    ))
                .min(session_exp),
                session_expires_at: session_exp,
            },
        ))
    }
    pub async fn active(&self, token: &str, user: Uuid) -> Result<bool, OidcError> {
        let form = {
            let mut form = url::form_urlencoded::Serializer::new(String::new());
            form.append_pair("token", token);
            form.finish()
        };
        let response = self
            .http
            .post(self.introspection.clone())
            .basic_auth(&self.client.client_id, self.client.client_secret.as_ref())
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(form)
            .send()
            .await
            .map_err(|_| OidcError::Unavailable)?;
        if !response.status().is_success() {
            return Err(OidcError::Unavailable);
        }
        let value: serde_json::Value = response
            .json()
            .await
            .map_err(|_| OidcError::InvalidResponse)?;
        validate_introspection(
            &value,
            &self.client.client_id,
            user,
            OffsetDateTime::now_utc(),
        )
    }
    pub fn identity_logout(&self) -> String {
        format!(
            "{}/oauth/logout",
            self.issuer.origin().ascii_serialization()
        )
    }
    pub async fn revoke(&self, token: &str) -> Result<(), OidcError> {
        let form = {
            let mut form = url::form_urlencoded::Serializer::new(String::new());
            form.append_pair("token", token);
            form.append_pair("token_type_hint", "refresh_token");
            form.finish()
        };
        let response = self
            .http
            .post(format!(
                "{}/oauth/revoke",
                self.issuer.origin().ascii_serialization()
            ))
            .basic_auth(&self.client.client_id, self.client.client_secret.as_ref())
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(form)
            .send()
            .await
            .map_err(|_| OidcError::Unavailable)?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(OidcError::Unavailable);
        }
        Ok(())
    }
}
impl Drop for Oidc {
    fn drop(&mut self) {
        if let Some(client) = Arc::get_mut(&mut self.client) {
            clear_client(client);
        }
    }
}
fn clear_client(client: &mut IdentityClient) {
    if let Some(secret) = &mut client.client_secret {
        secret.zeroize();
    }
    if let Some(pkce) = &mut client.pkce {
        match pkce {
            Pkce::S256(value) => {
                value.code_verifier.zeroize();
                value.code_challenge.zeroize();
            }
            Pkce::Plain(value) => value.zeroize(),
        }
    }
}
fn fixed_endpoint(value: &Url, issuer: &Url) -> Result<(), OidcError> {
    if value.origin() != issuer.origin()
        || !value.username().is_empty()
        || value.password().is_some()
        || value.fragment().is_some()
    {
        return Err(OidcError::InvalidResponse);
    }
    Ok(())
}

fn client_error(error: openid::error::ClientError) -> OidcError {
    match error {
        openid::error::ClientError::OAuth2(error)
            if error.error == openid::OAuth2ErrorCode::InvalidGrant =>
        {
            OidcError::InvalidGrant
        }
        openid::error::ClientError::Json(_) | openid::error::ClientError::MissingRefreshToken => {
            OidcError::InvalidResponse
        }
        _ => OidcError::Unavailable,
    }
}
fn session_claims(token: &str) -> Result<IdentityClaims, OidcError> {
    openid::Jws::<IdentityClaims, openid::Empty>::new_encoded(token)
        .unverified_payload()
        .map_err(|_| OidcError::InvalidResponse)
}
fn opaque_token(value: &str) -> bool {
    value.len() == 43
        && BASE64_URL_SAFE_NO_PAD
            .decode(value)
            .is_ok_and(|value| value.len() == 32)
}
fn validate_public_keys(raw: &serde_json::Value) -> Result<(), OidcError> {
    let keys = raw
        .get("keys")
        .and_then(serde_json::Value::as_array)
        .ok_or(OidcError::InvalidResponse)?;
    let mut identifiers = BTreeSet::new();
    if keys.is_empty() || keys.len() > 16 {
        return Err(OidcError::InvalidResponse);
    }
    for key in keys {
        let object = key.as_object().ok_or(OidcError::InvalidResponse)?;
        let kid = key["kid"]
            .as_str()
            .filter(|kid| !kid.is_empty() && kid.len() <= 128)
            .ok_or(OidcError::InvalidResponse)?;
        if key["kty"] != "RSA"
            || key["alg"] != "RS256"
            || key["use"] != "sig"
            || !identifiers.insert(kid)
            || object.keys().any(|name| {
                matches!(
                    name.as_str(),
                    "d" | "p" | "q" | "dp" | "dq" | "qi" | "oth" | "k" | "x5u"
                )
            })
        {
            return Err(OidcError::InvalidResponse);
        }
        let modulus = key["n"]
            .as_str()
            .and_then(|value| BASE64_URL_SAFE_NO_PAD.decode(value).ok())
            .ok_or(OidcError::InvalidResponse)?;
        let exponent = key["e"]
            .as_str()
            .and_then(|value| BASE64_URL_SAFE_NO_PAD.decode(value).ok())
            .ok_or(OidcError::InvalidResponse)?;
        if !(256..=1024).contains(&modulus.len())
            || modulus[0] < 128
            || exponent.as_slice() != [1, 0, 1]
        {
            return Err(OidcError::InvalidResponse);
        }
    }
    Ok(())
}
fn validate_introspection(
    value: &serde_json::Value,
    client: &str,
    user: Uuid,
    now: OffsetDateTime,
) -> Result<bool, OidcError> {
    let active = value
        .get("active")
        .and_then(serde_json::Value::as_bool)
        .ok_or(OidcError::InvalidResponse)?;
    if !active {
        return Ok(false);
    }
    let exp = value
        .get("exp")
        .and_then(serde_json::Value::as_i64)
        .ok_or(OidcError::InvalidResponse)?;
    let iat = value
        .get("iat")
        .and_then(serde_json::Value::as_i64)
        .ok_or(OidcError::InvalidResponse)?;
    let sub = value
        .get("sub")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok());
    let scope = value
        .get("scope")
        .and_then(serde_json::Value::as_str)
        .ok_or(OidcError::InvalidResponse)?;
    if value.get("client_id").and_then(serde_json::Value::as_str) != Some(client)
        || sub != Some(user)
        || value.get("token_type").and_then(serde_json::Value::as_str) != Some("Bearer")
        || iat < 0
        || iat > now.unix_timestamp().saturating_add(120)
        || exp <= now.unix_timestamp()
        || exp
            .checked_sub(iat)
            .is_none_or(|lifetime| lifetime <= 0 || lifetime > 300)
        || !scope
            .split_ascii_whitespace()
            .any(|scope| scope == "openid")
    {
        return Err(OidcError::InvalidResponse);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn public_key() -> serde_json::Value {
        let mut modulus = [1_u8; 256];
        modulus[0] = 128;
        json!({"kid":"test-public","kty":"RSA","alg":"RS256","use":"sig","n":BASE64_URL_SAFE_NO_PAD.encode(modulus),"e":"AQAB"})
    }
    #[test]
    fn public_keys_reject_private_material_algorithm_confusion_and_duplicate_ids() {
        assert!(validate_public_keys(&json!({"keys":[public_key()]})).is_ok());
        for name in ["d", "p", "q", "dp", "dq", "qi", "oth", "k", "x5u"] {
            let mut key = public_key();
            key[name] = json!("forbidden");
            assert!(validate_public_keys(&json!({"keys":[key]})).is_err());
        }
        assert!(validate_public_keys(&json!({"keys":[public_key(),public_key()]})).is_err());
        for (field, value) in [
            ("alg", "HS256"),
            ("use", "enc"),
            ("kty", "oct"),
            ("kid", ""),
            ("n", "AQ"),
            ("e", "Aw"),
        ] {
            let mut key = public_key();
            key[field] = json!(value);
            assert!(validate_public_keys(&json!({"keys":[key]})).is_err());
        }
    }
    #[test]
    fn introspection_requires_current_matching_identity_and_complete_active_schema()
    -> Result<(), Box<dyn std::error::Error>> {
        let now = OffsetDateTime::from_unix_timestamp(1800000000)?;
        let user = Uuid::new_v4();
        let valid = json!({"active":true,"scope":"openid profile email","client_id":"demo-a","sub":user,"iat":now.unix_timestamp(),"exp":now.unix_timestamp()+300,"token_type":"Bearer"});
        assert!(matches!(
            validate_introspection(&valid, "demo-a", user, now),
            Ok(true)
        ));
        assert!(matches!(
            validate_introspection(&json!({"active":false}), "demo-a", user, now),
            Ok(false)
        ));
        assert!(validate_introspection(&json!({"active":true}), "demo-a", user, now).is_err());
        for field in ["scope", "client_id", "sub", "iat", "exp", "token_type"] {
            let mut malformed = valid.clone();
            malformed
                .as_object_mut()
                .ok_or("missing fixture object")?
                .remove(field);
            assert!(validate_introspection(&malformed, "demo-a", user, now).is_err());
        }
        for (field, value) in [
            ("client_id", json!("demo-b")),
            ("sub", json!(Uuid::new_v4())),
            ("scope", json!("email")),
            ("exp", json!(now.unix_timestamp())),
            ("iat", json!(now.unix_timestamp() + 121)),
        ] {
            let mut malformed = valid.clone();
            malformed[field] = value;
            assert!(validate_introspection(&malformed, "demo-a", user, now).is_err());
        }
        Ok(())
    }
    #[test]
    fn endpoint_trust_boundary_blocks_cross_origin_credentials_and_fragments()
    -> Result<(), Box<dyn std::error::Error>> {
        let issuer = Url::parse("https://identity.example")?;
        for endpoint in [
            "http://identity.example/oauth/token",
            "https://other.example/oauth/token",
            "https://user@identity.example/oauth/token",
            "https://identity.example/oauth/token#fragment",
        ] {
            assert!(fixed_endpoint(&Url::parse(endpoint)?, &issuer).is_err());
        }
        assert!(
            fixed_endpoint(
                &Url::parse("https://identity.example/oauth/token")?,
                &issuer
            )
            .is_ok()
        );
        Ok(())
    }
}
