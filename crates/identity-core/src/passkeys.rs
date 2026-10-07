//! Fixed RP/origin WebAuthn verification. Only server-owned encrypted states are deserialized.
use serde_json::Value;
use std::fmt;
use url::Url;
use uuid::Uuid;
use webauthn_rs::prelude::*;
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasskeyError {
    Configuration,
    InvalidCredential,
    InvalidState,
}
impl fmt::Display for PasskeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Configuration => "invalid WebAuthn configuration",
            Self::InvalidCredential => "invalid WebAuthn credential",
            Self::InvalidState => "invalid server WebAuthn state",
        })
    }
}
impl std::error::Error for PasskeyError {}
#[derive(Clone)]
pub struct PasskeyEngine(Webauthn);
impl PasskeyEngine {
    pub fn new(issuer: &Url, rp_id: &str) -> Result<Self, PasskeyError> {
        if issuer.host_str() != Some(rp_id)
            || !matches!(issuer.scheme(), "http" | "https")
            || (issuer.scheme() == "http"
                && !matches!(issuer.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")))
            || issuer.path() != "/"
            || issuer.query().is_some()
            || issuer.fragment().is_some()
        {
            return Err(PasskeyError::Configuration);
        }
        WebauthnBuilder::new(rp_id, issuer)
            .map_err(|_| PasskeyError::Configuration)?
            .rp_name("统一身份中心")
            .build()
            .map(Self)
            .map_err(|_| PasskeyError::Configuration)
    }
    pub fn registration(
        &self,
        user: Uuid,
        email: &str,
        exclusions: Vec<Vec<u8>>,
    ) -> Result<(Value, Zeroizing<Vec<u8>>), PasskeyError> {
        let ids = exclusions.into_iter().map(CredentialID::from).collect();
        let (options, state) = self
            .0
            .start_passkey_registration(user, email, email, Some(ids))
            .map_err(|_| PasskeyError::InvalidCredential)?;
        let mut options = serde_json::to_value(options).map_err(|_| PasskeyError::InvalidState)?;
        options["publicKey"]["pubKeyCredParams"] =
            serde_json::json!([{"type":"public-key","alg":-7},{"type":"public-key","alg":-257}]);
        options["publicKey"]["authenticatorSelection"]["residentKey"] =
            Value::String("required".into());
        options["publicKey"]["authenticatorSelection"]["requireResidentKey"] = Value::Bool(true);
        Ok((
            options,
            Zeroizing::new(serde_json::to_vec(&state).map_err(|_| PasskeyError::InvalidState)?),
        ))
    }
    pub fn verify_registration(
        &self,
        credential: &Value,
        state: &[u8],
    ) -> Result<Value, PasskeyError> {
        let reg: RegisterPublicKeyCredential = serde_json::from_value(credential.clone())
            .map_err(|_| PasskeyError::InvalidCredential)?;
        if reg
            .extensions
            .cred_props
            .as_ref()
            .and_then(|props| props.rk)
            != Some(true)
        {
            return Err(PasskeyError::InvalidCredential);
        }
        let state: PasskeyRegistration =
            serde_json::from_slice(state).map_err(|_| PasskeyError::InvalidState)?;
        let key = self
            .0
            .finish_passkey_registration(&reg, &state)
            .map_err(|_| PasskeyError::InvalidCredential)?;
        if !matches!(
            key.cred_algorithm(),
            COSEAlgorithm::ES256 | COSEAlgorithm::RS256
        ) {
            return Err(PasskeyError::InvalidCredential);
        }
        serde_json::to_value(key).map_err(|_| PasskeyError::InvalidState)
    }
    pub fn credential_id(&self, key: &Value) -> Result<Vec<u8>, PasskeyError> {
        let key: Passkey =
            serde_json::from_value(key.clone()).map_err(|_| PasskeyError::InvalidState)?;
        Ok(key.cred_id().as_ref().to_vec())
    }
    pub fn discoverable_options(&self) -> Result<(Value, Zeroizing<Vec<u8>>), PasskeyError> {
        let (options, state) = self
            .0
            .start_discoverable_authentication()
            .map_err(|_| PasskeyError::InvalidCredential)?;
        let mut options = serde_json::to_value(options).map_err(|_| PasskeyError::InvalidState)?;
        if let Some(object) = options.as_object_mut() {
            object.remove("mediation");
        }
        Ok((
            options,
            Zeroizing::new(serde_json::to_vec(&state).map_err(|_| PasskeyError::InvalidState)?),
        ))
    }
    pub fn identify(&self, assertion: &Value) -> Result<(Uuid, Vec<u8>), PasskeyError> {
        let assertion: PublicKeyCredential = serde_json::from_value(assertion.clone())
            .map_err(|_| PasskeyError::InvalidCredential)?;
        let (user, id) = self
            .0
            .identify_discoverable_authentication(&assertion)
            .map_err(|_| PasskeyError::InvalidCredential)?;
        Ok((user, id.to_vec()))
    }
    pub fn verify_discoverable(
        &self,
        assertion: &Value,
        state: &[u8],
        key: &Value,
    ) -> Result<Value, PasskeyError> {
        let assertion: PublicKeyCredential = serde_json::from_value(assertion.clone())
            .map_err(|_| PasskeyError::InvalidCredential)?;
        let state: DiscoverableAuthentication =
            serde_json::from_slice(state).map_err(|_| PasskeyError::InvalidState)?;
        let mut key: Passkey =
            serde_json::from_value(key.clone()).map_err(|_| PasskeyError::InvalidState)?;
        if !matches!(
            key.cred_algorithm(),
            COSEAlgorithm::ES256 | COSEAlgorithm::RS256
        ) {
            return Err(PasskeyError::InvalidCredential);
        }
        let result = self
            .0
            .finish_discoverable_authentication(&assertion, state, &[DiscoverableKey::from(&key)])
            .map_err(|_| PasskeyError::InvalidCredential)?;
        if !result.user_verified() || key.update_credential(&result).is_none() {
            return Err(PasskeyError::InvalidCredential);
        }
        serde_json::to_value(key).map_err(|_| PasskeyError::InvalidState)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn options_enforce_fixed_rp_uv_and_resident_without_account_enumeration()
    -> Result<(), Box<dyn std::error::Error>> {
        let engine = PasskeyEngine::new(&Url::parse("http://localhost:5173")?, "localhost")?;
        let (options, state) =
            engine.registration(Uuid::new_v4(), "fixture@example.test", vec![])?;
        assert_eq!(
            options["publicKey"]["authenticatorSelection"]["residentKey"],
            "required"
        );
        assert_eq!(
            options["publicKey"]["authenticatorSelection"]["userVerification"],
            "required"
        );
        assert!(!state.is_empty());
        let (options, state) = engine.discoverable_options()?;
        assert_eq!(options["publicKey"]["userVerification"], "required");
        assert!(
            options["publicKey"]["allowCredentials"]
                .as_array()
                .is_none_or(|values| values.is_empty())
        );
        assert!(!state.is_empty());
        assert!(PasskeyEngine::new(&Url::parse("http://localhost:5173")?, "evil.test").is_err());
        assert!(engine.identify(&serde_json::json!({})).is_err());
        Ok(())
    }
}
