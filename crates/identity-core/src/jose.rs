//! Fixed RS256 signing and public-only key publication. No token header chooses an algorithm.
use crate::config::Config;
use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use openssl::pkey::PKey;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs,
};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoseError {
    InvalidConfiguration,
    InvalidClaims,
    SigningFailed,
    InvalidToken,
}

#[cfg(test)]
mod tests {
    use super::*;
    use openssl::rsa::Rsa;
    #[test]
    fn fixed_rs256_public_jwks_and_claim_validation() -> Result<(), Box<dyn std::error::Error>> {
        let pem = Zeroizing::new(Rsa::generate(2048)?.private_key_to_pem()?);
        let signer = Signer::from_pem(
            "https://identity.example".into(),
            "test-current".into(),
            &pem,
            None,
        )?;
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let mut claims = IdTokenClaims {
            iss: signer.issuer().into(),
            sub: Uuid::new_v4(),
            aud: "demo-a".into(),
            exp: now + 300,
            iat: now,
            nonce: Some("nonce_unit_example_1234".into()),
            auth_time: now,
            amr: vec!["pwd".into()],
            sid: Uuid::new_v4(),
        };
        let token = signer.sign(&claims)?;
        let decoded = signer.verify(&token, "demo-a")?;
        assert_eq!(decoded.sub, claims.sub);
        assert!(signer.verify(&token, "demo-b").is_err());
        let keys = signer.jwks();
        let public = keys["keys"][0].as_object().ok_or("public key missing")?;
        assert_eq!(public.get("alg"), Some(&Value::String("RS256".into())));
        assert!(
            public
                .keys()
                .all(|name| !matches!(name.as_str(), "d" | "p" | "q" | "qi"))
        );
        claims.iss = "https://evil.example".into();
        assert!(signer.sign(&claims).is_err());
        claims.iss = signer.issuer().into();
        claims.exp = now + 301;
        assert!(signer.sign(&claims).is_err());
        claims.exp = now - 1;
        assert!(signer.sign(&claims).is_err());
        let header = Header::new(Algorithm::HS256);
        let forged = jsonwebtoken::encode(
            &header,
            &claims,
            &EncodingKey::from_secret(b"unit fixture key"),
        )?;
        assert!(signer.verify(&forged, "demo-a").is_err());
        Ok(())
    }
    #[test]
    fn old_public_key_remains_verifiable_and_private_jwks_rejected()
    -> Result<(), Box<dyn std::error::Error>> {
        let pem = Zeroizing::new(Rsa::generate(2048)?.private_key_to_pem()?);
        let old = Signer::from_pem("https://identity.example".into(), "old".into(), &pem, None)?;
        let currentpem = Zeroizing::new(Rsa::generate(2048)?.private_key_to_pem()?);
        let previous = old.jwks().to_string();
        let current = Signer::from_pem(
            "https://identity.example".into(),
            "current".into(),
            &currentpem,
            Some(&previous),
        )?;
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let claims = IdTokenClaims {
            iss: old.issuer().into(),
            sub: Uuid::new_v4(),
            aud: "demo-a".into(),
            exp: now + 120,
            iat: now,
            nonce: None,
            auth_time: now,
            amr: vec!["user".into()],
            sid: Uuid::new_v4(),
        };
        assert!(current.verify(&old.sign(&claims)?, "demo-a").is_ok());
        let mut private = old.jwks();
        private["keys"][0]["d"] = Value::String("secret-invalid".into());
        assert!(
            Signer::from_pem(
                "https://identity.example".into(),
                "current".into(),
                &currentpem,
                Some(&private.to_string())
            )
            .is_err()
        );
        Ok(())
    }
}
impl fmt::Display for JoseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidConfiguration => "invalid signing key configuration",
            Self::InvalidClaims => "invalid identity claims",
            Self::SigningFailed => "identity signing unavailable",
            Self::InvalidToken => "invalid identity token",
        })
    }
}
impl std::error::Error for JoseError {}
#[derive(Serialize, Deserialize, Clone)]
pub struct IdTokenClaims {
    pub iss: String,
    pub sub: Uuid,
    pub aud: String,
    pub exp: i64,
    pub iat: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    pub auth_time: i64,
    pub amr: Vec<String>,
    pub sid: Uuid,
}
pub struct Signer {
    issuer: String,
    kid: String,
    key: EncodingKey,
    jwks: Value,
    verification: BTreeMap<String, DecodingKey>,
}
impl Signer {
    pub fn load(config: &Config) -> Result<Self, JoseError> {
        let pem = Zeroizing::new(
            fs::read(&config.signing_key_file).map_err(|_| JoseError::InvalidConfiguration)?,
        );
        let previous = if let Some(path) = &config.jwks_previous_file {
            Some(Zeroizing::new(
                fs::read_to_string(path).map_err(|_| JoseError::InvalidConfiguration)?,
            ))
        } else {
            None
        };
        Self::from_pem(
            config.issuer.origin().ascii_serialization(),
            config.signing_kid.clone(),
            &pem,
            previous.as_deref().map(String::as_str),
        )
    }
    fn from_pem(
        issuer: String,
        kid: String,
        pem: &[u8],
        previous: Option<&str>,
    ) -> Result<Self, JoseError> {
        if kid.is_empty()
            || kid.len() > 128
            || !kid
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        {
            return Err(JoseError::InvalidConfiguration);
        }
        let private =
            PKey::private_key_from_pem(pem).map_err(|_| JoseError::InvalidConfiguration)?;
        let rsa = private.rsa().map_err(|_| JoseError::InvalidConfiguration)?;
        if rsa.size() < 256 {
            return Err(JoseError::InvalidConfiguration);
        }
        let n = BASE64_URL_SAFE_NO_PAD.encode(rsa.n().to_vec());
        let e = BASE64_URL_SAFE_NO_PAD.encode(rsa.e().to_vec());
        let current = json!({"kty":"RSA","use":"sig","alg":"RS256","kid":kid,"n":n,"e":e});
        let key = EncodingKey::from_rsa_pem(pem).map_err(|_| JoseError::InvalidConfiguration)?;
        let mut keys = vec![current];
        if let Some(raw) = previous {
            let old: Value =
                serde_json::from_str(raw).map_err(|_| JoseError::InvalidConfiguration)?;
            let entries = old
                .get("keys")
                .and_then(Value::as_array)
                .ok_or(JoseError::InvalidConfiguration)?;
            keys.extend(entries.iter().cloned());
        }
        let mut seen = BTreeSet::new();
        let mut verification = BTreeMap::new();
        for public in &keys {
            let object = public.as_object().ok_or(JoseError::InvalidConfiguration)?;
            if object.keys().any(|field| {
                matches!(
                    field.as_str(),
                    "d" | "p" | "q" | "dp" | "dq" | "qi" | "oth" | "k"
                )
            }) || public["kty"] != "RSA"
                || public["alg"] != "RS256"
                || public["use"] != "sig"
            {
                return Err(JoseError::InvalidConfiguration);
            }
            let kid = public["kid"]
                .as_str()
                .filter(|id| !id.is_empty() && id.len() <= 128)
                .ok_or(JoseError::InvalidConfiguration)?;
            if !seen.insert(kid.to_owned()) {
                return Err(JoseError::InvalidConfiguration);
            }
            let n = public["n"]
                .as_str()
                .ok_or(JoseError::InvalidConfiguration)?;
            let e = public["e"]
                .as_str()
                .ok_or(JoseError::InvalidConfiguration)?;
            let nbytes = BASE64_URL_SAFE_NO_PAD
                .decode(n)
                .map_err(|_| JoseError::InvalidConfiguration)?;
            if nbytes.len() < 256 {
                return Err(JoseError::InvalidConfiguration);
            }
            verification.insert(
                kid.into(),
                DecodingKey::from_rsa_components(n, e)
                    .map_err(|_| JoseError::InvalidConfiguration)?,
            );
        }
        Ok(Self {
            issuer,
            kid,
            key,
            jwks: json!({"keys":keys}),
            verification,
        })
    }
    pub fn issuer(&self) -> &str {
        &self.issuer
    }
    pub fn jwks(&self) -> Value {
        self.jwks.clone()
    }
    pub fn sign(&self, claims: &IdTokenClaims) -> Result<Zeroizing<String>, JoseError> {
        if claims.iss != self.issuer
            || claims.aud.is_empty()
            || claims.aud.len() > 128
            || claims.iat < 0
            || claims.exp <= claims.iat
            || claims
                .iat
                .checked_add(300)
                .is_none_or(|maximum| claims.exp > maximum)
            || claims.auth_time > claims.iat
            || claims.auth_time < 0
            || claims.amr.is_empty()
            || claims
                .amr
                .iter()
                .any(|method| !matches!(method.as_str(), "pwd" | "otp" | "rcv" | "user" | "hwk"))
        {
            return Err(JoseError::InvalidClaims);
        }
        if claims
            .nonce
            .as_ref()
            .is_some_and(|value| !crate::oauth::valid_state(value))
        {
            return Err(JoseError::InvalidClaims);
        }
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(self.kid.clone());
        jsonwebtoken::encode(&header, claims, &self.key)
            .map(Zeroizing::new)
            .map_err(|_| JoseError::SigningFailed)
    }
    pub fn verify(&self, token: &str, audience: &str) -> Result<IdTokenClaims, JoseError> {
        let header = jsonwebtoken::decode_header(token).map_err(|_| JoseError::InvalidToken)?;
        if header.alg != Algorithm::RS256 {
            return Err(JoseError::InvalidToken);
        }
        let key = self
            .verification
            .get(header.kid.as_deref().ok_or(JoseError::InvalidToken)?)
            .ok_or(JoseError::InvalidToken)?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.leeway = 0;
        validation.set_issuer(&[&self.issuer]);
        validation.set_audience(&[audience]);
        validation.set_required_spec_claims(&["iss", "sub", "aud", "exp", "iat"]);
        let claims = jsonwebtoken::decode::<IdTokenClaims>(token, key, &validation)
            .map_err(|_| JoseError::InvalidToken)?
            .claims;
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        if claims.iat < 0
            || claims.iat > now.saturating_add(120)
            || claims.auth_time < 0
            || claims.auth_time > claims.iat
            || claims.exp <= claims.iat
            || claims
                .iat
                .checked_add(300)
                .is_none_or(|maximum| claims.exp > maximum)
            || claims.amr.is_empty()
            || claims
                .amr
                .iter()
                .any(|method| !matches!(method.as_str(), "pwd" | "otp" | "rcv" | "user" | "hwk"))
        {
            return Err(JoseError::InvalidToken);
        }
        Ok(claims)
    }
}
