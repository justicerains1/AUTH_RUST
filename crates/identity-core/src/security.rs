//! Local password rules, bounded Argon2id and purpose-bound secret cryptography.
//! No caller password or token is sent to a network service or formatted by Debug.
use aes_gcm::{
    Aes256Gcm,
    aead::{Aead, KeyInit, Payload, array::Array},
};
use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use base64::{Engine, prelude::BASE64_STANDARD, prelude::BASE64_URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest as ShaDigest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fmt, fs,
    path::Path,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;
use uuid::Uuid;
use zeroize::Zeroizing;

pub const ARGON2_MEMORY_KIB: u32 = 65_536;
pub const ARGON2_ITERATIONS: u32 = 3;
pub const ARGON2_LANES: u32 = 1;
pub const MAX_HASH_TASKS: usize = 4;
pub const HASH_QUEUE_TIMEOUT: Duration = Duration::from_millis(250);
pub const WEAK_PASSWORD_LIST_VERSION: &str = "seclists-913b327317496d062bcc7cace524aaad8a693be2";
pub const WEAK_PASSWORD_LIST_SHA256: &str =
    "68782d6a4a19a4768d5f15dd66bd534e7a33055cc755411e33f16d18c50fdcce";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecurityError {
    InvalidEmail,
    PasswordLength,
    WeakPassword,
    InvalidPasswordHash,
    InvalidConfiguration,
    RandomUnavailable,
    Busy,
    HashFailed,
    InvalidKey,
    InvalidEnvelope,
    AuthenticationFailed,
}
impl fmt::Display for SecurityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidEmail => "invalid email",
            Self::PasswordLength => {
                "password must contain 15 to 128 Unicode characters and at most 512 UTF-8 bytes"
            }
            Self::WeakPassword => "password is in the local weak-password list",
            Self::InvalidPasswordHash => "invalid stored password hash",
            Self::InvalidConfiguration => "invalid security configuration",
            Self::RandomUnavailable => "secure randomness unavailable",
            Self::Busy => "password hashing temporarily busy",
            Self::HashFailed => "password hashing unavailable",
            Self::InvalidKey => "invalid encryption key configuration",
            Self::InvalidEnvelope => "invalid encrypted envelope",
            Self::AuthenticationFailed => "encrypted data authentication failed",
        })
    }
}
impl std::error::Error for SecurityError {}

pub fn normalize_email(input: &str) -> Result<String, SecurityError> {
    if !input.is_ascii() || input.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(SecurityError::InvalidEmail);
    }
    let email = input.trim().to_ascii_lowercase();
    if email.len() > 254 {
        return Err(SecurityError::InvalidEmail);
    }
    let mut parts = email.split('@');
    let local = parts.next().ok_or(SecurityError::InvalidEmail)?;
    let domain = parts.next().ok_or(SecurityError::InvalidEmail)?;
    if parts.next().is_some()
        || local.is_empty()
        || local.len() > 64
        || local.starts_with('.')
        || local.ends_with('.')
        || local.contains("..")
        || !local
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".!#$%&'*+/=?^_`{|}~-".contains(&byte))
        || domain.is_empty()
        || domain.len() > 253
        || domain.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return Err(SecurityError::InvalidEmail);
    }
    Ok(email)
}

pub struct WeakPasswordList {
    entries: HashSet<&'static str>,
}
impl WeakPasswordList {
    pub fn bundled() -> &'static Self {
        static LIST: OnceLock<WeakPasswordList> = OnceLock::new();
        LIST.get_or_init(|| WeakPasswordList {
            entries: include_str!("../../../data/weak-passwords.txt")
                .lines()
                .collect(),
        })
    }
    pub fn contains(&self, candidate: &str) -> bool {
        self.entries.contains(candidate)
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

pub struct Password(Zeroizing<String>);
impl Password {
    /// Password creation rules only. Login verification intentionally accepts legacy shorter inputs.
    pub fn new(value: &str) -> Result<Self, SecurityError> {
        validate_password_shape(value)?;
        if WeakPasswordList::bundled().contains(value) {
            return Err(SecurityError::WeakPassword);
        }
        Ok(Self(Zeroizing::new(value.to_owned())))
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Password([REDACTED])")
    }
}
pub fn validate_password_shape(value: &str) -> Result<(), SecurityError> {
    if !(15..=128).contains(&value.chars().count()) || value.len() > 512 {
        return Err(SecurityError::PasswordLength);
    }
    Ok(())
}

fn argon2_current() -> Result<Argon2<'static>, SecurityError> {
    let params = Params::new(ARGON2_MEMORY_KIB, ARGON2_ITERATIONS, ARGON2_LANES, Some(32))
        .map_err(|_| SecurityError::InvalidConfiguration)?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}
fn hash_sync(password: &[u8]) -> Result<Zeroizing<String>, SecurityError> {
    let mut salt = Zeroizing::new([0u8; 16]);
    getrandom::fill(salt.as_mut()).map_err(|_| SecurityError::RandomUnavailable)?;
    let hash: PasswordHash = argon2_current()?
        .hash_password_with_salt(password, salt.as_ref())
        .map_err(|_| SecurityError::HashFailed)?;
    Ok(Zeroizing::new(hash.to_string()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PasswordVerification {
    pub valid: bool,
    pub needs_upgrade: bool,
}
fn verify_sync(password: &[u8], encoded: &str) -> Result<PasswordVerification, SecurityError> {
    let hash = PasswordHash::new(encoded).map_err(|_| SecurityError::InvalidPasswordHash)?;
    if hash.algorithm.as_str() != "argon2id" || !matches!(hash.version, Some(16 | 19)) {
        return Err(SecurityError::InvalidPasswordHash);
    }
    let params = Params::try_from(&hash).map_err(|_| SecurityError::InvalidPasswordHash)?;
    // Reject corrupt resource-amplification hashes; future cost upgrades must update this envelope.
    if params.m_cost() > 262_144 || params.t_cost() > 10 || params.p_cost() > 4 {
        return Err(SecurityError::InvalidPasswordHash);
    }
    let valid = match argon2_current()?.verify_password(password, &hash) {
        Ok(()) => true,
        Err(argon2::password_hash::Error::PasswordInvalid) => false,
        Err(_) => return Err(SecurityError::InvalidPasswordHash),
    };
    Ok(PasswordVerification {
        valid,
        needs_upgrade: valid
            && (params.m_cost() < ARGON2_MEMORY_KIB
                || params.t_cost() < ARGON2_ITERATIONS
                || hash.version != Some(19)
                || params.output_len().unwrap_or(32) < 32),
    })
}

#[derive(Default)]
struct PasswordMetrics {
    hashes: AtomicU64,
    verifications: AtomicU64,
    queue_timeouts: AtomicU64,
    hash_nanoseconds: AtomicU64,
    verification_nanoseconds: AtomicU64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PasswordMetricsSnapshot {
    pub hashes: u64,
    pub verifications: u64,
    pub queue_timeouts: u64,
    pub hash_nanoseconds: u64,
    pub verification_nanoseconds: u64,
    pub memory_kib: u32,
    pub iterations: u32,
    pub lanes: u32,
}

#[derive(Clone)]
pub struct PasswordService {
    permits: Arc<Semaphore>,
    dummy_hash: Arc<Zeroizing<String>>,
    metrics: Arc<PasswordMetrics>,
}
impl PasswordService {
    /// Process-wide budget and dummy. Independent callers cannot multiply Argon2 concurrency.
    pub async fn initialize(max_parallel: usize) -> Result<Self, SecurityError> {
        if !(1..=MAX_HASH_TASKS).contains(&max_parallel) {
            return Err(SecurityError::InvalidConfiguration);
        }
        static BUDGET: OnceLock<(usize, Arc<Semaphore>)> = OnceLock::new();
        static DUMMY: tokio::sync::OnceCell<Arc<Zeroizing<String>>> =
            tokio::sync::OnceCell::const_new();
        static METRICS: OnceLock<Arc<PasswordMetrics>> = OnceLock::new();
        let (configured, permits) =
            BUDGET.get_or_init(|| (max_parallel, Arc::new(Semaphore::new(max_parallel))));
        if *configured != max_parallel {
            return Err(SecurityError::InvalidConfiguration);
        }
        let permits = permits.clone();
        let dummy_hash = DUMMY
            .get_or_try_init(|| async {
                let permit =
                    tokio::time::timeout(HASH_QUEUE_TIMEOUT, permits.clone().acquire_owned())
                        .await
                        .map_err(|_| SecurityError::Busy)?
                        .map_err(|_| SecurityError::Busy)?;
                let dummy_password = Token::generate()?;
                let dummy = Zeroizing::new(dummy_password.expose().as_bytes().to_vec());
                let dummy_hash = tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    hash_sync(&dummy)
                })
                .await
                .map_err(|_| SecurityError::HashFailed)??;
                Ok::<_, SecurityError>(Arc::new(dummy_hash))
            })
            .await?
            .clone();
        Ok(Self {
            permits,
            dummy_hash,
            metrics: METRICS
                .get_or_init(|| Arc::new(PasswordMetrics::default()))
                .clone(),
        })
    }

    async fn permit(&self) -> Result<tokio::sync::OwnedSemaphorePermit, SecurityError> {
        match tokio::time::timeout(HASH_QUEUE_TIMEOUT, self.permits.clone().acquire_owned()).await {
            Ok(Ok(permit)) => Ok(permit),
            _ => {
                self.metrics.queue_timeouts.fetch_add(1, Ordering::Relaxed);
                Err(SecurityError::Busy)
            }
        }
    }

    pub async fn hash(&self, password: &Password) -> Result<Zeroizing<String>, SecurityError> {
        self.hash_verified(password.expose()).await
    }

    /// Re-encode an already verified legacy password without applying new-password policy.
    /// Login must verify the current PHC first; creation/reset/change still require Password::new.
    pub async fn hash_verified(&self, password: &str) -> Result<Zeroizing<String>, SecurityError> {
        if password.len() > 512 || password.chars().count() > 128 {
            return Err(SecurityError::PasswordLength);
        }
        let permit = self.permit().await?;
        let input = Zeroizing::new(password.as_bytes().to_vec());
        let metrics = self.metrics.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let start = Instant::now();
            let result = hash_sync(&input);
            metrics.hashes.fetch_add(1, Ordering::Relaxed);
            metrics
                .hash_nanoseconds
                .fetch_add(elapsed_nanos(start), Ordering::Relaxed);
            result
        })
        .await
        .map_err(|_| SecurityError::HashFailed)?
    }

    pub async fn verify(
        &self,
        password: &str,
        encoded: &str,
    ) -> Result<PasswordVerification, SecurityError> {
        if password.len() > 512 || password.chars().count() > 128 || encoded.len() > 1024 {
            return Err(SecurityError::PasswordLength);
        }
        let permit = self.permit().await?;
        let input = Zeroizing::new(password.as_bytes().to_vec());
        let encoded = Zeroizing::new(encoded.to_owned());
        let metrics = self.metrics.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let start = Instant::now();
            let result = verify_sync(&input, &encoded);
            metrics.verifications.fetch_add(1, Ordering::Relaxed);
            metrics
                .verification_nanoseconds
                .fetch_add(elapsed_nanos(start), Ordering::Relaxed);
            result
        })
        .await
        .map_err(|_| SecurityError::HashFailed)?
    }

    pub async fn verify_unknown(&self, password: &str) -> Result<(), SecurityError> {
        self.verify(password, &self.dummy_hash).await?;
        Ok(())
    }

    pub fn metrics(&self) -> PasswordMetricsSnapshot {
        PasswordMetricsSnapshot {
            hashes: self.metrics.hashes.load(Ordering::Relaxed),
            verifications: self.metrics.verifications.load(Ordering::Relaxed),
            queue_timeouts: self.metrics.queue_timeouts.load(Ordering::Relaxed),
            hash_nanoseconds: self.metrics.hash_nanoseconds.load(Ordering::Relaxed),
            verification_nanoseconds: self
                .metrics
                .verification_nanoseconds
                .load(Ordering::Relaxed),
            memory_kib: ARGON2_MEMORY_KIB,
            iterations: ARGON2_ITERATIONS,
            lanes: ARGON2_LANES,
        }
    }
}
fn elapsed_nanos(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

pub struct Token(Zeroizing<String>);
impl Token {
    pub fn generate() -> Result<Self, SecurityError> {
        random_encoded::<32>().map(Self)
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token([REDACTED])")
    }
}
pub struct RecoveryCode(Zeroizing<String>);
impl RecoveryCode {
    pub fn generate() -> Result<Self, SecurityError> {
        random_encoded::<16>().map(Self)
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for RecoveryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecoveryCode([REDACTED])")
    }
}
fn random_encoded<const N: usize>() -> Result<Zeroizing<String>, SecurityError> {
    let mut bytes = Zeroizing::new([0u8; N]);
    getrandom::fill(bytes.as_mut()).map_err(|_| SecurityError::RandomUnavailable)?;
    Ok(Zeroizing::new(
        BASE64_URL_SAFE_NO_PAD.encode(bytes.as_ref()),
    ))
}
pub fn token_digest(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}
pub fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    bool::from(left.ct_eq(right))
}
pub fn keyed_account_digest(key: &[u8; 32], purpose: &str, value: &str) -> [u8; 32] {
    let purpose_length = (purpose.len() as u64).to_be_bytes();
    let value_length = (value.len() as u64).to_be_bytes();
    hmac_sha256(
        key,
        &[
            b"identity-platform:hmac:v1\0",
            &purpose_length,
            purpose.as_bytes(),
            &value_length,
            value.as_bytes(),
        ],
    )
}
fn hmac_sha256(key: &[u8; 32], parts: &[&[u8]]) -> [u8; 32] {
    let mut padded_key = Zeroizing::new([0u8; 64]);
    padded_key[..32].copy_from_slice(key);
    let mut mac = <Hmac<Sha256> as hmac::KeyInit>::new(&Array(*padded_key));
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AeadEnvelope {
    pub kid: String,
    pub nonce: String,
    pub ciphertext: String,
}
impl fmt::Debug for AeadEnvelope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AeadEnvelope")
            .field("kid", &self.kid)
            .field("ciphertext", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}
pub struct AeadKeyRing {
    active_kid: String,
    keys: BTreeMap<String, Zeroizing<[u8; 32]>>,
}
impl fmt::Debug for AeadKeyRing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AeadKeyRing")
            .field("active_kid", &self.active_kid)
            .field("keys", &"[REDACTED]")
            .finish()
    }
}
impl AeadKeyRing {
    pub fn new(active_kid: &str, keys: BTreeMap<String, [u8; 32]>) -> Result<Self, SecurityError> {
        if !valid_kid(active_kid)
            || !keys.contains_key(active_kid)
            || keys.is_empty()
            || keys
                .iter()
                .any(|(kid, key)| !valid_kid(kid) || key.iter().all(|byte| *byte == 0))
        {
            return Err(SecurityError::InvalidKey);
        }
        Ok(Self {
            active_kid: active_kid.to_owned(),
            keys: keys
                .into_iter()
                .map(|(kid, key)| (kid, Zeroizing::new(key)))
                .collect(),
        })
    }
    pub fn load_file(path: &Path, active_kid: &str) -> Result<Self, SecurityError> {
        let metadata = fs::metadata(path).map_err(|_| SecurityError::InvalidKey)?;
        if !metadata.is_file() || metadata.len() > 1024 * 1024 {
            return Err(SecurityError::InvalidKey);
        }
        let contents =
            Zeroizing::new(fs::read_to_string(path).map_err(|_| SecurityError::InvalidKey)?);
        let crate::config::EncryptionKeys(encoded) =
            serde_json::from_str(&contents).map_err(|_| SecurityError::InvalidKey)?;
        let mut keys = BTreeMap::new();
        for (kid, value) in encoded {
            let raw = Zeroizing::new(
                BASE64_STANDARD
                    .decode(value.as_bytes())
                    .map_err(|_| SecurityError::InvalidKey)?,
            );
            let key: [u8; 32] = raw
                .as_slice()
                .try_into()
                .map_err(|_| SecurityError::InvalidKey)?;
            keys.insert(kid, key);
        }
        Self::new(active_kid, keys)
    }
    pub fn derive_hmac_key(&self, purpose: &str) -> Result<Zeroizing<[u8; 32]>, SecurityError> {
        validate_purpose(purpose)?;
        let key = self
            .keys
            .get(&self.active_kid)
            .ok_or(SecurityError::InvalidKey)?;
        Ok(Zeroizing::new(keyed_account_digest(
            key,
            "derived-hmac-key",
            purpose,
        )))
    }
    pub fn encrypt(
        &self,
        user_id: Uuid,
        purpose: &str,
        plaintext: &[u8],
    ) -> Result<AeadEnvelope, SecurityError> {
        let aad = aad(user_id, purpose, &self.active_kid)?;
        let key = self
            .keys
            .get(&self.active_kid)
            .ok_or(SecurityError::InvalidKey)?;
        let cipher = Aes256Gcm::new(&Array(**key));
        let mut nonce = [0u8; 12];
        getrandom::fill(&mut nonce).map_err(|_| SecurityError::RandomUnavailable)?;
        let ciphertext = cipher
            .encrypt(
                &Array(nonce),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| SecurityError::AuthenticationFailed)?;
        Ok(AeadEnvelope {
            kid: self.active_kid.clone(),
            nonce: BASE64_URL_SAFE_NO_PAD.encode(nonce),
            ciphertext: BASE64_URL_SAFE_NO_PAD.encode(ciphertext),
        })
    }
    pub fn decrypt(
        &self,
        user_id: Uuid,
        purpose: &str,
        envelope: &AeadEnvelope,
    ) -> Result<Zeroizing<Vec<u8>>, SecurityError> {
        if !valid_kid(&envelope.kid)
            || envelope.nonce.len() != 16
            || envelope.ciphertext.len() > 2 * 1024 * 1024
        {
            return Err(SecurityError::InvalidEnvelope);
        }
        let aad = aad(user_id, purpose, &envelope.kid)?;
        let key = self
            .keys
            .get(&envelope.kid)
            .ok_or(SecurityError::InvalidKey)?;
        let nonce: [u8; 12] = BASE64_URL_SAFE_NO_PAD
            .decode(&envelope.nonce)
            .map_err(|_| SecurityError::InvalidEnvelope)?
            .as_slice()
            .try_into()
            .map_err(|_| SecurityError::InvalidEnvelope)?;
        let ciphertext = BASE64_URL_SAFE_NO_PAD
            .decode(&envelope.ciphertext)
            .map_err(|_| SecurityError::InvalidEnvelope)?;
        let cipher = Aes256Gcm::new(&Array(**key));
        cipher
            .decrypt(
                &Array(nonce),
                Payload {
                    msg: &ciphertext,
                    aad: &aad,
                },
            )
            .map(Zeroizing::new)
            .map_err(|_| SecurityError::AuthenticationFailed)
    }
}
fn valid_kid(kid: &str) -> bool {
    !kid.is_empty()
        && kid.len() <= 128
        && kid
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}
fn validate_purpose(purpose: &str) -> Result<(), SecurityError> {
    if purpose.is_empty()
        || purpose.len() > 128
        || !purpose
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
    {
        return Err(SecurityError::InvalidConfiguration);
    }
    Ok(())
}
fn aad(user_id: Uuid, purpose: &str, kid: &str) -> Result<Vec<u8>, SecurityError> {
    validate_purpose(purpose)?;
    let mut aad = b"identity-platform:aead:v1\0".to_vec();
    aad.extend_from_slice(user_id.as_bytes());
    for value in [purpose, kid] {
        aad.extend_from_slice(&(value.len() as u64).to_be_bytes());
        aad.extend_from_slice(value.as_bytes());
    }
    Ok(aad)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_normalization_preserves_aliases_and_rejects_invalid_domains() {
        assert_eq!(
            normalize_email(" User.Name+tag@Example.COM "),
            Ok("user.name+tag@example.com".into())
        );
        for input in [
            "a\n@example.com",
            "用户@example.com",
            "a@@example.com",
            ".a@example.com",
            "a..b@example.com",
            "a@-example.com",
            "a@example-.com",
            "a@example..com",
            "a@exa_mple.com",
            "a @example.com",
        ] {
            assert!(normalize_email(input).is_err());
        }
        assert!(normalize_email(&format!("{}@example.com", "a".repeat(65))).is_err());
        assert!(normalize_email(&format!("a@{}.com", "a".repeat(64))).is_err());
        let maximum = format!(
            "{}@{}.{}.{}",
            "a".repeat(64),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(61)
        );
        assert_eq!(maximum.len(), 254);
        assert!(normalize_email(&maximum).is_ok());
        assert!(normalize_email(&format!("{maximum}x")).is_err());
    }

    #[test]
    fn unicode_password_limits_do_not_trim_or_force_character_classes() {
        assert!(Password::new(&"界".repeat(15)).is_ok());
        assert!(Password::new(&"🦀".repeat(128)).is_ok());
        assert!(Password::new("               ").is_ok());
        assert!(Password::new(&"界".repeat(14)).is_err());
        assert!(Password::new(&"🦀".repeat(129)).is_err());
        assert!(Password::new(&"a".repeat(129)).is_err());
        let password = Password::new(" a safe phrase with spaces ");
        assert!(password.is_ok());
        if let Ok(password) = password {
            assert!(password.expose().starts_with(' '));
            assert!(!format!("{password:?}").contains("safe phrase"));
        }
    }

    #[test]
    fn bundled_weak_list_matches_pinned_raw_bytes_and_rejects_an_allowed_length_entry() {
        let raw = include_bytes!("../../../data/weak-passwords.txt");
        let digest = Sha256::digest(raw);
        let actual: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(actual, WEAK_PASSWORD_LIST_SHA256);
        assert_eq!(WeakPasswordList::bundled().len(), 10001);
        let candidate = WeakPasswordList::bundled()
            .entries
            .iter()
            .find(|entry| entry.chars().count() >= 15);
        assert!(candidate.is_some());
        if let Some(candidate) = candidate {
            assert!(matches!(
                Password::new(candidate),
                Err(SecurityError::WeakPassword)
            ));
        }
    }

    #[test]
    fn random_tokens_recovery_and_comparison_have_correct_entropy_and_redaction()
    -> Result<(), SecurityError> {
        let mut tokens = HashSet::new();
        for _ in 0..128 {
            let token = Token::generate()?;
            assert_eq!(token.expose().len(), 43);
            assert_eq!(
                BASE64_URL_SAFE_NO_PAD
                    .decode(token.expose())
                    .map_err(|_| SecurityError::InvalidEnvelope)?
                    .len(),
                32
            );
            assert!(tokens.insert(token_digest(token.expose())));
            assert_eq!(format!("{token:?}"), "Token([REDACTED])");
        }
        let code = RecoveryCode::generate()?;
        assert_eq!(
            BASE64_URL_SAFE_NO_PAD
                .decode(code.expose())
                .map_err(|_| SecurityError::InvalidEnvelope)?
                .len(),
            16
        );
        assert!(constant_time_equal(&[7; 32], &[7; 32]));
        assert!(!constant_time_equal(&[7; 32], &[8; 32]));
        assert!(!constant_time_equal(&[7; 31], &[7; 32]));
        Ok(())
    }

    #[test]
    fn hmac_matches_rfc4231_and_separates_purpose_boundaries() {
        let mut key = [0u8; 32];
        key[..20].fill(0x0b);
        let digest = hmac_sha256(&key, &[b"Hi There"]);
        let actual: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(
            actual,
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        assert_ne!(
            keyed_account_digest(&key, "ab", "c"),
            keyed_account_digest(&key, "a", "bc")
        );
        assert_ne!(
            keyed_account_digest(&key, "email", "target"),
            keyed_account_digest(&key, "ip", "target")
        );
    }

    #[test]
    fn aead_authenticates_user_purpose_nonce_ciphertext_and_key_version()
    -> Result<(), SecurityError> {
        let user = Uuid::new_v4();
        let ring = AeadKeyRing::new(
            "current",
            BTreeMap::from([("current".into(), [9; 32]), ("previous".into(), [8; 32])]),
        )?;
        let envelope = ring.encrypt(user, "totp.seed", b"SECRET_UNIT_SENTINEL")?;
        assert_eq!(
            ring.decrypt(user, "totp.seed", &envelope)?.as_slice(),
            b"SECRET_UNIT_SENTINEL"
        );
        assert!(
            ring.decrypt(Uuid::new_v4(), "totp.seed", &envelope)
                .is_err()
        );
        assert!(ring.decrypt(user, "mail.action", &envelope).is_err());
        let mut tampered = envelope.clone();
        tampered.kid = "previous".into();
        assert!(ring.decrypt(user, "totp.seed", &tampered).is_err());
        let mut tampered = envelope.clone();
        let mut ciphertext = BASE64_URL_SAFE_NO_PAD
            .decode(&tampered.ciphertext)
            .map_err(|_| SecurityError::InvalidEnvelope)?;
        ciphertext[0] ^= 1;
        tampered.ciphertext = BASE64_URL_SAFE_NO_PAD.encode(ciphertext);
        assert!(ring.decrypt(user, "totp.seed", &tampered).is_err());
        let mut tampered = envelope.clone();
        let mut nonce = BASE64_URL_SAFE_NO_PAD
            .decode(&tampered.nonce)
            .map_err(|_| SecurityError::InvalidEnvelope)?;
        nonce[0] ^= 1;
        tampered.nonce = BASE64_URL_SAFE_NO_PAD.encode(nonce);
        assert!(ring.decrypt(user, "totp.seed", &tampered).is_err());
        let another = ring.encrypt(user, "totp.seed", b"SECRET_UNIT_SENTINEL")?;
        assert_ne!(envelope.nonce, another.nonce);
        assert_ne!(envelope.ciphertext, another.ciphertext);
        let old = AeadKeyRing::new("previous", BTreeMap::from([("previous".into(), [8; 32])]))?;
        let old_envelope = old.encrypt(user, "totp.seed", b"old-secret")?;
        assert_eq!(
            ring.decrypt(user, "totp.seed", &old_envelope)?.as_slice(),
            b"old-secret"
        );
        assert!(AeadKeyRing::new("zero", BTreeMap::from([("zero".into(), [0; 32])])).is_err());
        assert_ne!(
            *ring.derive_hmac_key("email-limit")?,
            *ring.derive_hmac_key("ip-limit")?
        );
        assert!(!format!("{ring:?} {envelope:?}").contains("SECRET_UNIT_SENTINEL"));
        assert_ne!(aad(user, "ab", "c")?, aad(user, "a", "bc")?);
        Ok(())
    }

    #[test]
    fn duplicate_key_versions_are_rejected_before_crypto_loading()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory =
            std::env::temp_dir().join(format!("identity-crypto-unit-{}", Uuid::new_v4()));
        fs::create_dir(&directory)?;
        let path = directory.join("keys.json");
        let encoded = BASE64_STANDARD.encode([7u8; 32]);
        fs::write(
            &path,
            format!("{{\"active\":\"{encoded}\",\"active\":\"{encoded}\"}}"),
        )?;
        let result = AeadKeyRing::load_file(&path, "active");
        fs::remove_dir_all(&directory)?;
        assert!(matches!(result, Err(SecurityError::InvalidKey)));
        Ok(())
    }

    #[tokio::test]
    async fn argon2_real_hash_legacy_verify_dummy_reuse_and_global_queue_budget()
    -> Result<(), SecurityError> {
        let service = PasswordService::initialize(4).await?;
        let other = PasswordService::initialize(4).await?;
        assert!(Arc::ptr_eq(&service.permits, &other.permits));
        assert!(Arc::ptr_eq(&service.dummy_hash, &other.dummy_hash));
        assert!(PasswordService::initialize(5).await.is_err());
        let password = Password::new("unique Unicode 安全 phrase 2026")?;
        let encoded = service.hash(&password).await?;
        let parsed = PasswordHash::new(&encoded).map_err(|_| SecurityError::InvalidPasswordHash)?;
        let params = Params::try_from(&parsed).map_err(|_| SecurityError::InvalidPasswordHash)?;
        assert_eq!(
            (params.m_cost(), params.t_cost(), params.p_cost()),
            (65536, 3, 1)
        );
        assert_eq!(parsed.algorithm.as_str(), "argon2id");
        assert_eq!(
            service.verify(password.expose(), &encoded).await?,
            PasswordVerification {
                valid: true,
                needs_upgrade: false
            }
        );
        assert!(
            !service
                .verify("wrong phrase with sufficient length", &encoded)
                .await?
                .valid
        );
        let legacy = tokio::task::spawn_blocking(move || {
            let parameters = Params::new(8192, 1, 1, Some(32))
                .map_err(|_| SecurityError::InvalidConfiguration)?;
            let legacy = Argon2::new(Algorithm::Argon2id, Version::V0x13, parameters);
            let hash: PasswordHash = legacy
                .hash_password_with_salt(b"legacy passphrase", &[3u8; 16])
                .map_err(|_| SecurityError::HashFailed)?;
            Ok::<_, SecurityError>(Zeroizing::new(hash.to_string()))
        })
        .await
        .map_err(|_| SecurityError::HashFailed)??;
        assert_eq!(
            service.verify("legacy passphrase", &legacy).await?,
            PasswordVerification {
                valid: true,
                needs_upgrade: true
            }
        );
        assert!(
            service
                .verify("legacy passphrase", "malformed PHC")
                .await
                .is_err()
        );
        // Successful legacy login can upgrade a short historical password without accepting
        // it for new registrations. Encoding upgrades never change credential_version.
        assert!(Password::new("legacy-short").is_err());
        let upgraded = service.hash_verified("legacy-short").await?;
        assert!(service.verify("legacy-short", &upgraded).await?.valid);
        assert!(
            !service
                .verify("legacy-short", &upgraded)
                .await?
                .needs_upgrade
        );
        assert!(service.hash_verified(&"a".repeat(129)).await.is_err());
        let dummy_before = token_digest(&service.dummy_hash);
        service
            .verify_unknown("unknown account password phrase")
            .await?;
        service
            .verify_unknown("different unknown password phrase")
            .await?;
        assert_eq!(dummy_before, token_digest(&service.dummy_hash));
        // Occupy all process-wide permits without timing real hashes; the queue deadline is actual.
        let held = service
            .permits
            .clone()
            .acquire_many_owned(4)
            .await
            .map_err(|_| SecurityError::Busy)?;
        let start = Instant::now();
        assert!(matches!(
            other.verify_unknown("a valid candidate passphrase").await,
            Err(SecurityError::Busy)
        ));
        assert!(start.elapsed() >= HASH_QUEUE_TIMEOUT);
        drop(held);
        service
            .verify_unknown("a valid candidate passphrase")
            .await?;
        let metrics = service.metrics();
        assert!(metrics.hashes >= 1 && metrics.verifications >= 6 && metrics.queue_timeouts >= 1);
        assert!(metrics.hash_nanoseconds > 0 && metrics.verification_nanoseconds > 0);
        Ok(())
    }
}
