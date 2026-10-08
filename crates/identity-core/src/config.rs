//! Validates the fixed deployment trust boundary before opening a listener.
//! Errors report configuration names only, including when a secret URL is invalid.
use base64::{
    Engine,
    prelude::{BASE64_STANDARD, BASE64_URL_SAFE_NO_PAD},
};
use ipnet::IpNet;
use serde::de::{self, MapAccess, Visitor};
use std::{
    collections::BTreeMap,
    fmt, fs,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};
use url::Url;
use zeroize::{Zeroize, Zeroizing};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Environment {
    Development,
    Test,
    Production,
}

pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

pub struct Config {
    pub environment: Environment,
    pub bind: SocketAddr,
    pub issuer: Url,
    pub rp_id: String,
    pub database_url: Secret,
    pub redis_url: Secret,
    pub signing_key_file: PathBuf,
    pub signing_kid: String,
    pub jwks_previous_file: Option<PathBuf>,
    pub encryption_keys_file: PathBuf,
    pub metrics_token_digest: Option<[u8; 32]>,
    pub active_encryption_kid: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_username: Option<Secret>,
    pub smtp_password: Option<Secret>,
    pub smtp_from: String,
    pub smtp_tls: SmtpTls,
    pub trusted_proxy_cidrs: Vec<IpNet>,
    pub database_pool_max: u32,
    pub argon2_parallelism_limit: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmtpTls {
    Required,
    DisabledForDevelopment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfigError {
    pub field: &'static str,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid or missing configuration: {}",
            self.field
        )
    }
}

impl std::error::Error for ConfigError {}

fn invalid(field: &'static str) -> ConfigError {
    ConfigError { field }
}

fn required(values: &BTreeMap<String, String>, field: &'static str) -> Result<String, ConfigError> {
    values
        .get(field)
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .ok_or(invalid(field))
}

fn secret_file(
    path: &str,
    field: &'static str,
    production: bool,
) -> Result<Zeroizing<String>, ConfigError> {
    let metadata = fs::metadata(path).map_err(|_| invalid(field))?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return Err(invalid(field));
    }
    #[cfg(unix)]
    if production {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(invalid(field));
        }
    }
    #[cfg(not(unix))]
    let _ = production;
    let contents = Zeroizing::new(fs::read_to_string(path).map_err(|_| invalid(field))?);
    if contents.trim().is_empty() {
        return Err(invalid(field));
    }
    Ok(contents)
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

fn issuer_origin(value: &str, production: bool) -> Result<Url, ConfigError> {
    let issuer = Url::parse(value).map_err(|_| invalid("ISSUER"))?;
    let origin = issuer.origin().ascii_serialization();
    // Compare the original input so URL normalization cannot erase paths or whitespace.
    if !matches!(issuer.scheme(), "http" | "https")
        || issuer.host_str().is_none()
        || (value != origin && value != format!("{origin}/"))
        || (production && issuer.scheme() != "https")
        || (issuer.scheme() == "http"
            && !matches!(issuer.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")))
    {
        return Err(invalid("ISSUER"));
    }
    Ok(issuer)
}

fn domain_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value.is_ascii()
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn smtp_host_valid(value: &str) -> bool {
    value.parse::<IpAddr>().is_ok() || domain_name(value)
}

fn smtp_from_valid(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    value.len() <= 254
        && !local.is_empty()
        && local.len() <= 64
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".!#$%&'*+-/=?^_`{|}~".contains(&byte))
        && domain_name(domain)
}

fn smtp_username_value(value: Option<&String>) -> Result<Option<Secret>, ConfigError> {
    value
        .filter(|value| !value.is_empty())
        .map(|value| {
            if value.trim().is_empty() || value.chars().any(char::is_control) {
                return Err(invalid("SMTP_USERNAME"));
            }
            Ok(Secret(Zeroizing::new(value.clone())))
        })
        .transpose()
}

fn validate_previous_jwks(text: &str, signing_kid: &str) -> Result<(), ConfigError> {
    let field = "JWKS_PREVIOUS_FILE";
    let jwks: serde_json::Value = serde_json::from_str(text).map_err(|_| invalid(field))?;
    let keys = jwks
        .get("keys")
        .and_then(serde_json::Value::as_array)
        .ok_or(invalid(field))?;
    let mut seen = std::collections::BTreeSet::new();
    seen.insert(signing_kid);
    for key in keys {
        let object = key.as_object().ok_or(invalid(field))?;
        let kid = object
            .get("kid")
            .and_then(serde_json::Value::as_str)
            .ok_or(invalid(field))?;
        if !identifier(kid)
            || !seen.insert(kid)
            || object.keys().any(|name| {
                matches!(
                    name.as_str(),
                    "d" | "p" | "q" | "dp" | "dq" | "qi" | "oth" | "k"
                )
            })
            || object.get("kty").and_then(serde_json::Value::as_str) != Some("RSA")
            || object.get("alg").and_then(serde_json::Value::as_str) != Some("RS256")
            || object.get("use").and_then(serde_json::Value::as_str) != Some("sig")
        {
            return Err(invalid(field));
        }
        let decode = |name| {
            let encoded = object
                .get(name)
                .and_then(serde_json::Value::as_str)
                .ok_or(invalid(field))?;
            BASE64_URL_SAFE_NO_PAD
                .decode(encoded)
                .map_err(|_| invalid(field))
        };
        let modulus = decode("n")?;
        let exponent = decode("e")?;
        if !(256..=1024).contains(&modulus.len())
            || modulus.first() == Some(&0)
            || modulus.last().is_none_or(|byte| byte % 2 == 0)
            || exponent.is_empty()
            || exponent.len() > 4
            || exponent.first() == Some(&0)
        {
            return Err(invalid(field));
        }
        let exponent_value = exponent
            .iter()
            .fold(0_u32, |value, byte| (value << 8) | u32::from(*byte));
        if exponent_value < 3 || exponent_value % 2 == 0 {
            return Err(invalid(field));
        }
        let jwk: jsonwebtoken::jwk::Jwk =
            serde_json::from_value(key.clone()).map_err(|_| invalid(field))?;
        jsonwebtoken::DecodingKey::from_jwk(&jwk).map_err(|_| invalid(field))?;
    }
    Ok(())
}

pub(crate) struct EncryptionKeys(pub(crate) BTreeMap<String, Zeroizing<String>>);

impl<'de> serde::Deserialize<'de> for EncryptionKeys {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct KeysVisitor;

        impl<'de> Visitor<'de> for KeysVisitor {
            type Value = EncryptionKeys;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a map of unique encryption key versions")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut keys = BTreeMap::new();
                while let Some((kid, value)) = map.next_entry::<String, String>()? {
                    let value = Zeroizing::new(value);
                    if keys.contains_key(&kid) {
                        // Never include a key ID or secret in a parser diagnostic.
                        return Err(de::Error::custom("duplicate encryption key version"));
                    }
                    keys.insert(kid, value);
                }
                Ok(EncryptionKeys(keys))
            }
        }

        deserializer.deserialize_map(KeysVisitor)
    }
}

fn validate_encryption_keys(text: &str, active_kid: &str) -> Result<(), ConfigError> {
    let field = "ENCRYPTION_KEYS_FILE";
    let EncryptionKeys(keys) = serde_json::from_str(text).map_err(|_| invalid(field))?;
    if keys.is_empty() {
        return Err(invalid(field));
    }
    if !keys.contains_key(active_kid) {
        return Err(invalid("ACTIVE_ENCRYPTION_KID"));
    }
    for (kid, encoded) in &keys {
        let bytes = Zeroizing::new(
            BASE64_STANDARD
                .decode(encoded.as_bytes())
                .map_err(|_| invalid(field))?,
        );
        if !identifier(kid) || bytes.len() != 32 || bytes.iter().all(|byte| *byte == 0) {
            return Err(invalid(field));
        }
    }
    Ok(())
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let mut values = BTreeMap::new();
        for field in [
            "APP_ENV",
            "BIND",
            "ISSUER",
            "RP_ID",
            "DATABASE_URL",
            "REDIS_URL",
            "SIGNING_KEY_FILE",
            "SIGNING_KID",
            "JWKS_PREVIOUS_FILE",
            "ENCRYPTION_KEYS_FILE",
            "METRICS_TOKEN_FILE",
            "ACTIVE_ENCRYPTION_KID",
            "SMTP_HOST",
            "SMTP_PORT",
            "SMTP_USERNAME",
            "SMTP_PASSWORD_FILE",
            "SMTP_FROM",
            "SMTP_TLS",
            "SMTP_TLS_VERIFY",
            "TRUSTED_PROXY_CIDRS",
            "DATABASE_POOL_MAX",
            "ARGON2_PARALLELISM_LIMIT",
            "DEV_SEED",
            "SEED_ACCEPTANCE",
            "DEBUG_ROUTES",
            "COOKIE_SECURE",
            "COOKIE_HTTP_ONLY",
            "COOKIE_PATH",
            "COOKIE_SAME_SITE",
            "COOKIE_DOMAIN",
        ] {
            match std::env::var(field) {
                Ok(value) => {
                    values.insert(field.to_string(), value);
                }
                Err(std::env::VarError::NotPresent) => {}
                Err(std::env::VarError::NotUnicode(_)) => {
                    values.values_mut().for_each(Zeroize::zeroize);
                    return Err(invalid(field));
                }
            }
        }
        let result = Self::from_values(&values);
        values.values_mut().for_each(Zeroize::zeroize);
        result
    }

    pub fn from_values(values: &BTreeMap<String, String>) -> Result<Self, ConfigError> {
        let environment = match required(values, "APP_ENV")?.as_str() {
            "development" => Environment::Development,
            "test" => Environment::Test,
            "production" => Environment::Production,
            _ => return Err(invalid("APP_ENV")),
        };
        let production = environment == Environment::Production;
        let metrics_token_digest = values
            .get("METRICS_TOKEN_FILE")
            .filter(|value| !value.is_empty())
            .map(|path| {
                crate::observability::load_metrics_digest(std::path::Path::new(path), production)
                    .map_err(|_| invalid("METRICS_TOKEN_FILE"))
            })
            .transpose()?;
        let bind = values
            .get("BIND")
            .map(String::as_str)
            .unwrap_or("127.0.0.1:8080")
            .parse()
            .map_err(|_| invalid("BIND"))?;
        let issuer_text = required(values, "ISSUER")?;
        let issuer = issuer_origin(&issuer_text, production)?;
        let rp_id = required(values, "RP_ID")?;
        if Some(rp_id.as_str()) != issuer.host_str() {
            return Err(invalid("RP_ID"));
        }
        let database_text = required(values, "DATABASE_URL")?;
        let database = Url::parse(&database_text).map_err(|_| invalid("DATABASE_URL"))?;
        if !matches!(database.scheme(), "postgres" | "postgresql")
            || database.host_str().is_none()
            || database.username().is_empty()
            || database.path().len() <= 1
            || database.fragment().is_some()
        {
            return Err(invalid("DATABASE_URL"));
        }
        let redis_text = required(values, "REDIS_URL")?;
        let redis = Url::parse(&redis_text).map_err(|_| invalid("REDIS_URL"))?;
        if !matches!(redis.scheme(), "redis" | "rediss")
            || redis.host_str().is_none()
            || redis.fragment().is_some()
        {
            return Err(invalid("REDIS_URL"));
        }
        let signing_key_file = required(values, "SIGNING_KEY_FILE")?;
        let signing_kid = required(values, "SIGNING_KID")?;
        if !identifier(&signing_kid) {
            return Err(invalid("SIGNING_KID"));
        }
        let signing_pem = secret_file(&signing_key_file, "SIGNING_KEY_FILE", production)?;
        let signing_key = jsonwebtoken::EncodingKey::from_rsa_pem(signing_pem.as_bytes())
            .map_err(|_| invalid("SIGNING_KEY_FILE"))?;
        // aws-lc validates the RSA private key and supported modulus size. The probe is discarded.
        jsonwebtoken::encode(
            &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
            &serde_json::json!({}),
            &signing_key,
        )
        .map_err(|_| invalid("SIGNING_KEY_FILE"))?;
        let jwks_previous_file = values
            .get("JWKS_PREVIOUS_FILE")
            .filter(|value| !value.is_empty())
            .map(|path| {
                let text = secret_file(path, "JWKS_PREVIOUS_FILE", false)?;
                validate_previous_jwks(&text, &signing_kid)?;
                Ok(PathBuf::from(path))
            })
            .transpose()?;
        let encryption_keys_file = required(values, "ENCRYPTION_KEYS_FILE")?;
        let active_encryption_kid = required(values, "ACTIVE_ENCRYPTION_KID")?;
        if !identifier(&active_encryption_kid) {
            return Err(invalid("ACTIVE_ENCRYPTION_KID"));
        }
        let encryption_text =
            secret_file(&encryption_keys_file, "ENCRYPTION_KEYS_FILE", production)?;
        validate_encryption_keys(&encryption_text, &active_encryption_kid)?;
        let smtp_host = required(values, "SMTP_HOST")?;
        if !smtp_host_valid(&smtp_host) {
            return Err(invalid("SMTP_HOST"));
        }
        let smtp_port = required(values, "SMTP_PORT")?
            .parse::<u16>()
            .map_err(|_| invalid("SMTP_PORT"))?;
        if smtp_port == 0 {
            return Err(invalid("SMTP_PORT"));
        }
        let smtp_from = required(values, "SMTP_FROM")?;
        if !smtp_from_valid(&smtp_from) {
            return Err(invalid("SMTP_FROM"));
        }
        let smtp_tls = match required(values, "SMTP_TLS")?.as_str() {
            "required" => SmtpTls::Required,
            "disabled"
                if !production
                    && matches!(
                        smtp_host.as_str(),
                        "localhost" | "127.0.0.1" | "::1" | "mailpit"
                    ) =>
            {
                SmtpTls::DisabledForDevelopment
            }
            _ => return Err(invalid("SMTP_TLS")),
        };
        if values
            .get("SMTP_TLS_VERIFY")
            .is_some_and(|value| value != "true")
        {
            return Err(invalid("SMTP_TLS_VERIFY"));
        }
        let smtp_username = smtp_username_value(values.get("SMTP_USERNAME"))?;
        let smtp_password = values
            .get("SMTP_PASSWORD_FILE")
            .filter(|value| !value.is_empty())
            .map(|path| secret_file(path, "SMTP_PASSWORD_FILE", production))
            .transpose()?;
        if production && smtp_username.is_none() {
            return Err(invalid("SMTP_USERNAME"));
        }
        if smtp_username.is_some() != smtp_password.is_some() {
            return Err(invalid("SMTP_PASSWORD_FILE"));
        }
        if production {
            for field in ["DEV_SEED", "SEED_ACCEPTANCE", "DEBUG_ROUTES"] {
                if values
                    .get(field)
                    .is_some_and(|value| !matches!(value.as_str(), "false" | "0" | ""))
                {
                    return Err(invalid(field));
                }
            }
        }
        // Cookie settings are invariants, rather than switches which can weaken production.
        for (field, expected) in [
            ("COOKIE_HTTP_ONLY", "true"),
            ("COOKIE_PATH", "/"),
            ("COOKIE_SAME_SITE", "Lax"),
        ] {
            if values.get(field).is_some_and(|value| value != expected) {
                return Err(invalid(field));
            }
        }
        if values
            .get("COOKIE_DOMAIN")
            .is_some_and(|value| !value.is_empty())
        {
            return Err(invalid("COOKIE_DOMAIN"));
        }
        if production
            && values
                .get("COOKIE_SECURE")
                .is_some_and(|value| value != "true")
        {
            return Err(invalid("COOKIE_SECURE"));
        }
        if !production
            && issuer.scheme() != "http"
            && values
                .get("COOKIE_SECURE")
                .is_some_and(|value| value != "true")
        {
            return Err(invalid("COOKIE_SECURE"));
        }
        let trusted_proxy_cidrs = values
            .get("TRUSTED_PROXY_CIDRS")
            .filter(|value| !value.trim().is_empty())
            .map(|value| {
                value
                    .split(',')
                    .map(|entry| {
                        entry
                            .trim()
                            .parse::<IpNet>()
                            .map_err(|_| invalid("TRUSTED_PROXY_CIDRS"))
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default();
        let database_pool_max = values
            .get("DATABASE_POOL_MAX")
            .map(String::as_str)
            .unwrap_or("32")
            .parse::<u32>()
            .map_err(|_| invalid("DATABASE_POOL_MAX"))?;
        if database_pool_max == 0 || database_pool_max > 256 {
            return Err(invalid("DATABASE_POOL_MAX"));
        }
        let argon2_parallelism_limit = values
            .get("ARGON2_PARALLELISM_LIMIT")
            .map(String::as_str)
            .unwrap_or("4")
            .parse::<usize>()
            .map_err(|_| invalid("ARGON2_PARALLELISM_LIMIT"))?;
        if argon2_parallelism_limit == 0 || argon2_parallelism_limit > 4 {
            return Err(invalid("ARGON2_PARALLELISM_LIMIT"));
        }
        Ok(Self {
            environment,
            bind,
            issuer,
            rp_id,
            database_url: Secret(Zeroizing::new(database_text)),
            redis_url: Secret(Zeroizing::new(redis_text)),
            signing_key_file: signing_key_file.into(),
            signing_kid,
            jwks_previous_file,
            encryption_keys_file: encryption_keys_file.into(),
            metrics_token_digest,
            active_encryption_kid,
            smtp_host,
            smtp_port,
            smtp_from,
            smtp_tls,
            smtp_username,
            smtp_password: smtp_password.map(Secret),
            trusted_proxy_cidrs,
            database_pool_max,
            argon2_parallelism_limit,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn early_config() -> BTreeMap<String, String> {
        [
            ("APP_ENV", "production"),
            ("ISSUER", "https://identity.example"),
            ("RP_ID", "identity.example"),
            (
                "DATABASE_URL",
                "postgres://identity:redaction-sentinel@localhost/identity_test",
            ),
            ("REDIS_URL", "redis://localhost:6379"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
    }

    fn rejects(values: &BTreeMap<String, String>, field: &str) {
        match Config::from_values(values) {
            Err(error) => {
                assert_eq!(error.field, field);
                assert!(!error.to_string().contains("redaction-sentinel"));
            }
            Ok(_) => panic!("unsafe configuration accepted"),
        }
    }

    #[test]
    fn production_origin_is_fixed_and_https() {
        for issuer in [
            "http://identity.example",
            "https://identity.example/path",
            "https://identity.example?x=1",
            "https://identity.example#x",
            "https://user:redaction-sentinel@identity.example",
            "https://identity.example/a/..",
            "https://identity.example/%2e/",
            " https://identity.example ",
            "https://identity.example\n",
            "https://identity.example\\",
        ] {
            let mut values = early_config();
            values.insert("ISSUER".into(), issuer.into());
            rejects(&values, "ISSUER");
        }
    }

    #[test]
    fn invalid_environment_rp_or_secret_connection_never_echoes_values() {
        let mut values = early_config();
        values.insert("APP_ENV".into(), "redaction-sentinel".into());
        rejects(&values, "APP_ENV");
        values = early_config();
        values.insert("RP_ID".into(), "wrong.example".into());
        rejects(&values, "RP_ID");
        values = early_config();
        values.insert("DATABASE_URL".into(), "redaction-sentinel".into());
        rejects(&values, "DATABASE_URL");
        values = early_config();
        rejects(&values, "SIGNING_KEY_FILE");
    }

    #[test]
    fn secret_debug_is_redacted() {
        let secret = Secret(Zeroizing::new("redaction-sentinel".into()));
        assert_eq!(format!("{secret:?}"), "[REDACTED]");
    }

    #[test]
    fn canonical_origins_with_optional_root_slash_are_accepted() {
        for issuer in [
            "https://identity.example",
            "https://identity.example/",
            "https://identity.example:8443",
        ] {
            assert!(issuer_origin(issuer, true).is_ok());
        }
        assert!(issuer_origin("http://localhost:5173", false).is_ok());
        assert!(issuer_origin("http://[::1]:5173", false).is_ok());
        assert!(issuer_origin("http://other.example", false).is_err());
    }

    #[test]
    fn smtp_host_and_mailbox_are_plain_addresses() {
        for host in [
            "smtp.example",
            "mailpit",
            "localhost",
            "127.0.0.1",
            "::1",
            "2001:db8::1",
        ] {
            assert!(smtp_host_valid(host));
        }
        for host in [
            "",
            "user:pass@host",
            "smtp.example:587",
            "smtp.example/path",
            "smtp.example?x",
            "bad_host",
            "-bad.example",
            "bad..example",
            "smtp.example\n",
        ] {
            assert!(!smtp_host_valid(host));
        }
        for mailbox in [
            "no-reply@localhost",
            "first.last+tag@example.com",
            "a@sub.example",
        ] {
            assert!(smtp_from_valid(mailbox));
        }
        for mailbox in [
            "\"bad@example",
            "Name <a@example>",
            ".a@example",
            "a.@example",
            "a..b@example",
            "a@@example",
            "a@-bad.example",
            "a@bad..example",
            "a@example\r\nBcc:x@example",
            "名字@example",
        ] {
            assert!(!smtp_from_valid(mailbox));
        }
        assert!(!smtp_from_valid(&format!("{}@example.com", "a".repeat(65))));
    }

    #[test]
    fn smtp_credentials_reject_blank_accounts_and_control_characters() {
        for username in [" ", "\t", "user\n", "user\0"] {
            let error = smtp_username_value(Some(&username.to_string()));
            assert!(matches!(
                error,
                Err(ConfigError {
                    field: "SMTP_USERNAME"
                })
            ));
        }
        assert!(matches!(smtp_username_value(None), Ok(None)));
        assert!(matches!(
            smtp_username_value(Some(&String::new())),
            Ok(None)
        ));
        assert!(matches!(
            smtp_username_value(Some(&"mail-user".to_string())),
            Ok(Some(_))
        ));
    }

    fn jwks_text(modulus: &[u8], exponent: &[u8]) -> String {
        serde_json::json!({"keys": [{
            "kid": "previous", "kty": "RSA", "alg": "RS256", "use": "sig",
            "n": BASE64_URL_SAFE_NO_PAD.encode(modulus),
            "e": BASE64_URL_SAFE_NO_PAD.encode(exponent),
        }]})
        .to_string()
    }

    #[test]
    fn previous_jwks_checks_unsigned_rsa_components_and_public_only_policy() {
        let modulus = vec![0x81; 256];
        let valid = jwks_text(&modulus, &[1, 0, 1]);
        assert!(validate_previous_jwks(&valid, "current").is_ok());
        assert!(validate_previous_jwks(&valid, "previous").is_err());
        for exponent in [&[][..], &[0], &[1], &[2], &[0, 3], &[1, 0, 0, 0, 1]] {
            assert!(validate_previous_jwks(&jwks_text(&modulus, exponent), "current").is_err());
        }
        for bad_modulus in [
            vec![],
            vec![0x81; 255],
            vec![0x81; 1025],
            {
                let mut m = modulus.clone();
                m[0] = 0;
                m
            },
            {
                let mut m = modulus.clone();
                m[255] = 0x80;
                m
            },
        ] {
            assert!(validate_previous_jwks(&jwks_text(&bad_modulus, &[3]), "current").is_err());
        }
        for field in ["n", "e"] {
            for bad in ["", "!!!", "AQAB="] {
                let mut document: serde_json::Value =
                    serde_json::from_str(&valid).unwrap_or_default();
                document["keys"][0][field] = bad.into();
                assert!(validate_previous_jwks(&document.to_string(), "current").is_err());
            }
        }
        let mut private: serde_json::Value = serde_json::from_str(&valid).unwrap_or_default();
        private["keys"][0]["d"] = "redaction-sentinel".into();
        assert!(validate_previous_jwks(&private.to_string(), "current").is_err());
        let duplicate =
            serde_json::json!({"keys": [private["keys"][0].clone(), private["keys"][0].clone()]})
                .to_string();
        assert!(validate_previous_jwks(&duplicate, "current").is_err());
    }

    #[test]
    fn encryption_map_rejects_duplicate_versions_and_invalid_secret_material() {
        let key = BASE64_STANDARD.encode([7; 32]);
        let valid = format!("{{\"active\":\"{key}\"}}");
        assert!(validate_encryption_keys(&valid, "active").is_ok());
        assert!(validate_encryption_keys(&valid, "absent").is_err());
        let duplicate = format!("{{\"active\":\"{key}\",\"active\":\"{key}\"}}");
        assert_eq!(
            validate_encryption_keys(&duplicate, "active"),
            Err(invalid("ENCRYPTION_KEYS_FILE"))
        );
        for encoded in [
            String::new(),
            "redaction-sentinel".into(),
            BASE64_STANDARD.encode([0; 32]),
            BASE64_STANDARD.encode([7; 31]),
        ] {
            let document = serde_json::json!({"active": encoded}).to_string();
            let result = validate_encryption_keys(&document, "active");
            assert_eq!(result, Err(invalid("ENCRYPTION_KEYS_FILE")));
        }
    }
}
