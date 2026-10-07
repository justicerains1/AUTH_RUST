//! Fixed RFC 6238 policy and single-display recovery material. Database owns replay prevention.
use crate::security::{RecoveryCode, SecurityError, token_digest};
use std::fmt;
use totp_rs::{Algorithm, Builder, Secret, Totp};
use zeroize::Zeroizing;

pub struct TotpSecret(Totp);
impl fmt::Debug for TotpSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TotpSecret([REDACTED])")
    }
}
impl TotpSecret {
    pub fn generate() -> Result<Self, SecurityError> {
        let mut bytes = Zeroizing::new([0u8; 20]);
        getrandom::fill(bytes.as_mut()).map_err(|_| SecurityError::RandomUnavailable)?;
        Self::from_bytes(bytes.as_ref())
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SecurityError> {
        if bytes.len() < 20 || bytes.len() > 64 {
            return Err(SecurityError::InvalidConfiguration);
        }
        let totp = Builder::new()
            .with_algorithm(Algorithm::SHA1)
            .with_digits(6)
            .with_step_duration(30)
            .with_skew(1)
            .with_secret(Secret::new(bytes.to_vec().into_boxed_slice()))
            .build()
            .map_err(|_| SecurityError::InvalidConfiguration)?;
        Ok(Self(totp))
    }
    pub fn from_base32(value: &str) -> Result<Self, SecurityError> {
        let secret =
            Secret::try_from_base32(value).map_err(|_| SecurityError::InvalidConfiguration)?;
        Self::from_bytes(secret.as_bytes())
    }
    pub fn base32(&self) -> Zeroizing<String> {
        Zeroizing::new(self.0.secret().to_base32())
    }
    pub fn bytes(&self) -> &[u8] {
        self.0.secret().as_bytes()
    }
    pub fn code_at(&self, unix_seconds: i64) -> Result<String, SecurityError> {
        let time = u64::try_from(unix_seconds).map_err(|_| SecurityError::InvalidConfiguration)?;
        Ok(self.0.generate(time).to_string())
    }
    pub fn matching_step(&self, code: &str, unix_seconds: i64) -> Option<i64> {
        if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        self.0
            .check(code, u64::try_from(unix_seconds).ok()?)
            .and_then(|step| i64::try_from(step).ok())
    }
    pub fn otpauth_uri(&self, account: &str) -> Result<Zeroizing<String>, SecurityError> {
        let configured = Builder::new()
            .with_algorithm(Algorithm::SHA1)
            .with_digits(6)
            .with_step_duration(30)
            .with_skew(1)
            .with_secret(Secret::new(self.bytes().to_vec().into_boxed_slice()))
            .with_issuer(Some("统一身份中心"))
            .with_account_name(account)
            .build()
            .map_err(|_| SecurityError::InvalidConfiguration)?;
        configured
            .to_url()
            .map(Zeroizing::new)
            .map_err(|_| SecurityError::InvalidConfiguration)
    }
}
pub struct RecoveryCodes(Zeroizing<Vec<String>>);
impl RecoveryCodes {
    pub fn generate() -> Result<Self, SecurityError> {
        let mut codes = Zeroizing::new(Vec::new());
        for _ in 0..10 {
            codes.push(RecoveryCode::generate()?.expose().to_owned());
        }
        Ok(Self(codes))
    }
    pub fn expose(&self) -> &[String] {
        &self.0
    }
    pub fn digests(&self) -> Vec<[u8; 32]> {
        self.0.iter().map(|code| token_digest(code)).collect()
    }
}
impl fmt::Debug for RecoveryCodes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecoveryCodes([REDACTED])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rfc6238_sha1_vectors_and_exact_step_boundaries() -> Result<(), SecurityError> {
        let secret = TotpSecret::from_bytes(b"12345678901234567890")?;
        for (time, expected) in [
            (59, "287082"),
            (1111111109, "081804"),
            (1111111111, "050471"),
            (1234567890, "005924"),
            (2000000000, "279037"),
            (20000000000, "353130"),
        ] {
            assert_eq!(secret.code_at(time)?, expected);
            assert_eq!(secret.matching_step(expected, time), Some(time / 30));
        }
        let code = secret.code_at(60)?;
        assert_eq!(secret.matching_step(&code, 30), Some(2));
        assert_eq!(secret.matching_step(&code, 90), Some(2));
        assert_eq!(secret.matching_step(&code, 120), None);
        assert_eq!(secret.matching_step("00000", 60), None);
        assert_eq!(secret.matching_step("00000a", 60), None);
        assert_eq!(secret.matching_step(&code, -1), None);
        Ok(())
    }
    #[test]
    fn material_is_random_minimum160bit_and_roundtrips_without_debug() -> Result<(), SecurityError>
    {
        let a = TotpSecret::generate()?;
        let b = TotpSecret::generate()?;
        assert_ne!(a.bytes(), b.bytes());
        assert_eq!(a.bytes().len(), 20);
        let restored = TotpSecret::from_base32(&a.base32())?;
        assert_eq!(a.bytes(), restored.bytes());
        assert_eq!(format!("{a:?}"), "TotpSecret([REDACTED])");
        let codes = RecoveryCodes::generate()?;
        assert_eq!(codes.expose().len(), 10);
        assert!(codes.expose().iter().all(|code| code.len() == 22));
        let unique: std::collections::HashSet<_> = codes.digests().into_iter().collect();
        assert_eq!(unique.len(), 10);
        assert_eq!(format!("{codes:?}"), "RecoveryCodes([REDACTED])");
        Ok(())
    }
}
