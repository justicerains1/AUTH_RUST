//! Independent client configuration. Only named fields appear in diagnostics.
use identity_core::{config::Environment, security::AeadKeyRing};
use std::{
    collections::BTreeMap,
    fmt, fs,
    net::SocketAddr,
    path::{Path, PathBuf},
};
use url::Url;
use zeroize::{Zeroize, Zeroizing};

pub struct BffConfig {
    pub environment: Environment,
    pub bind: SocketAddr,
    pub public_origin: Url,
    pub issuer: Url,
    pub client_id: String,
    pub client_secret: Zeroizing<String>,
    pub database_url: Zeroizing<String>,
    pub namespace: String,
    pub cookie_name: String,
    pub flow_cookie_name: String,
    pub csrf_cookie_name: String,
    pub encryption_keys_file: PathBuf,
    pub active_encryption_kid: String,
    pub issuer_connect_host: Option<String>,
}
#[derive(Debug)]
pub struct ConfigError(pub &'static str);
impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid or missing BFF configuration: {}", self.0)
    }
}
impl std::error::Error for ConfigError {}
impl BffConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let mut values = BTreeMap::new();
        for name in [
            "APP_ENV",
            "BIND",
            "BFF_PUBLIC_ORIGIN",
            "ISSUER",
            "BFF_CLIENT_ID",
            "BFF_CLIENT_SECRET_FILE",
            "DATABASE_URL",
            "BFF_NAMESPACE",
            "BFF_COOKIE_NAME",
            "ENCRYPTION_KEYS_FILE",
            "ACTIVE_ENCRYPTION_KID",
            "BFF_ISSUER_CONNECT_HOST",
        ] {
            match std::env::var(name) {
                Ok(value) => {
                    values.insert(name.into(), value);
                }
                Err(std::env::VarError::NotPresent) => {}
                Err(_) => {
                    values.values_mut().for_each(Zeroize::zeroize);
                    return Err(ConfigError(name));
                }
            }
        }
        let result = Self::from_values(&values);
        values.values_mut().for_each(Zeroize::zeroize);
        result
    }
    pub fn from_values(values: &BTreeMap<String, String>) -> Result<Self, ConfigError> {
        let required = |name| {
            values
                .get(name)
                .filter(|value| !value.trim().is_empty())
                .cloned()
                .ok_or(ConfigError(name))
        };
        let environment = match required("APP_ENV")?.as_str() {
            "development" => Environment::Development,
            "test" => Environment::Test,
            "production" => Environment::Production,
            _ => return Err(ConfigError("APP_ENV")),
        };
        let origin = |name| {
            let raw = required(name)?;
            let url = Url::parse(&raw).map_err(|_| ConfigError(name))?;
            if !matches!(url.scheme(), "http" | "https")
                || raw != url.origin().ascii_serialization()
                || environment == Environment::Production && url.scheme() != "https"
                || url.scheme() == "http"
                    && !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
            {
                return Err(ConfigError(name));
            }
            Ok(url)
        };
        let public_origin = origin("BFF_PUBLIC_ORIGIN")?;
        let issuer = origin("ISSUER")?;
        let client_id = required("BFF_CLIENT_ID")?;
        if client_id.len() > 128
            || !client_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err(ConfigError("BFF_CLIENT_ID"));
        }
        let secret_file = required("BFF_CLIENT_SECRET_FILE")?;
        validate_file(
            Path::new(&secret_file),
            "BFF_CLIENT_SECRET_FILE",
            environment,
            1024,
        )?;
        let secret = Zeroizing::new(
            fs::read_to_string(secret_file).map_err(|_| ConfigError("BFF_CLIENT_SECRET_FILE"))?,
        );
        let client_secret = Zeroizing::new(secret.trim_end().to_string());
        if client_secret.len() < 43
            || client_secret.len() > 512
            || !client_secret
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
        {
            return Err(ConfigError("BFF_CLIENT_SECRET_FILE"));
        }
        let namespace = required("BFF_NAMESPACE")?;
        if namespace.is_empty()
            || namespace.len() > 64
            || !namespace.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte)
            })
        {
            return Err(ConfigError("BFF_NAMESPACE"));
        }
        let cookie_name = required("BFF_COOKIE_NAME")?;
        if cookie_name.len() > 128
            || !cookie_name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
            || environment == Environment::Production
                && (!cookie_name.starts_with("__Host-") || cookie_name.len() <= 7)
        {
            return Err(ConfigError("BFF_COOKIE_NAME"));
        }
        let issuer_connect_host = values
            .get("BFF_ISSUER_CONNECT_HOST")
            .filter(|value| !value.is_empty())
            .cloned();
        if issuer_connect_host
            .as_deref()
            .is_some_and(|value| value != "identity-web")
            || environment == Environment::Production && issuer_connect_host.is_some()
        {
            return Err(ConfigError("BFF_ISSUER_CONNECT_HOST"));
        }
        let bind = required("BIND")?.parse().map_err(|_| ConfigError("BIND"))?;
        let database_url = Zeroizing::new(required("DATABASE_URL")?);
        let database = Url::parse(&database_url).map_err(|_| ConfigError("DATABASE_URL"))?;
        if !matches!(database.scheme(), "postgres" | "postgresql")
            || database.host_str().is_none()
            || database.username().is_empty()
            || database.path().len() <= 1
            || database.fragment().is_some()
        {
            return Err(ConfigError("DATABASE_URL"));
        }
        let encryption_keys_file: PathBuf = required("ENCRYPTION_KEYS_FILE")?.into();
        validate_file(
            &encryption_keys_file,
            "ENCRYPTION_KEYS_FILE",
            environment,
            1024 * 1024,
        )?;
        let active_encryption_kid = required("ACTIVE_ENCRYPTION_KID")?;
        AeadKeyRing::load_file(&encryption_keys_file, &active_encryption_kid)
            .map_err(|_| ConfigError("ENCRYPTION_KEYS_FILE"))?;
        Ok(Self {
            environment,
            bind,
            public_origin,
            issuer,
            client_id,
            client_secret,
            database_url,
            namespace,
            flow_cookie_name: format!("{cookie_name}-flow"),
            csrf_cookie_name: format!("{cookie_name}-csrf"),
            cookie_name,
            encryption_keys_file,
            active_encryption_kid,
            issuer_connect_host,
        })
    }
    pub fn callback(&self) -> String {
        format!(
            "{}/bff/callback",
            self.public_origin.origin().ascii_serialization()
        )
    }
}
fn validate_file(
    path: &Path,
    field: &'static str,
    environment: Environment,
    limit: u64,
) -> Result<(), ConfigError> {
    let metadata = fs::metadata(path).map_err(|_| ConfigError(field))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > limit {
        return Err(ConfigError(field));
    }
    #[cfg(unix)]
    if environment == Environment::Production {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(ConfigError(field));
        }
    }
    #[cfg(not(unix))]
    let _ = environment;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, prelude::BASE64_STANDARD};
    struct Fixture {
        directory: PathBuf,
        values: BTreeMap<String, String>,
    }
    impl Fixture {
        fn new() -> Result<Self, Box<dyn std::error::Error>> {
            let directory =
                std::env::temp_dir().join(format!("bff-config-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&directory)?;
            let secret = directory.join("client-secret");
            fs::write(&secret, "A".repeat(43))?;
            let keys = directory.join("keys.json");
            fs::write(
                &keys,
                serde_json::json!({"test-key":BASE64_STANDARD.encode([7_u8;32])}).to_string(),
            )?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                for file in [&secret, &keys] {
                    fs::set_permissions(file, fs::Permissions::from_mode(0o600))?;
                }
            }
            let mut values: BTreeMap<String, String> = [
                ("APP_ENV", "test"),
                ("BIND", "127.0.0.1:8082"),
                ("BFF_PUBLIC_ORIGIN", "http://localhost:5174"),
                ("ISSUER", "http://localhost:5173"),
                ("BFF_CLIENT_ID", "demo-a"),
                (
                    "DATABASE_URL",
                    "postgres://fixture:private@localhost/identity_test",
                ),
                ("BFF_NAMESPACE", "demo_a"),
                ("BFF_COOKIE_NAME", "demo-a-session"),
                ("ACTIVE_ENCRYPTION_KID", "test-key"),
            ]
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();
            values.insert(
                "BFF_CLIENT_SECRET_FILE".into(),
                secret.to_string_lossy().into(),
            );
            values.insert("ENCRYPTION_KEYS_FILE".into(), keys.to_string_lossy().into());
            Ok(Self { directory, values })
        }
        fn production(&mut self) {
            for (key, value) in [
                ("APP_ENV", "production"),
                ("BFF_PUBLIC_ORIGIN", "https://app.example"),
                ("ISSUER", "https://identity.example"),
                ("BFF_COOKIE_NAME", "__Host-demo-a-session"),
            ] {
                self.values.insert(key.into(), value.into());
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.values.values_mut().for_each(Zeroize::zeroize);
            let _ = fs::remove_dir_all(&self.directory);
        }
    }
    #[test]
    fn config_rejects_invalid_trust_names_database_and_key_material_without_secret_diagnostics()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = Fixture::new()?;
        assert!(BffConfig::from_values(&fixture.values).is_ok());
        for (field, value) in [
            ("BFF_PUBLIC_ORIGIN", "http://other.example"),
            ("ISSUER", "http://localhost:5173/path"),
            ("BFF_CLIENT_ID", "reserved:id"),
            ("BFF_NAMESPACE", "BadNamespace"),
            ("BFF_COOKIE_NAME", "bad;cookie"),
            ("DATABASE_URL", "private-redaction-sentinel"),
            ("ACTIVE_ENCRYPTION_KID", "unknown"),
            ("BFF_ISSUER_CONNECT_HOST", "attacker.example"),
        ] {
            let mut values = fixture.values.clone();
            values.insert(field.into(), value.into());
            let error = BffConfig::from_values(&values)
                .err()
                .ok_or("configuration incorrectly accepted")?;
            assert!(!error.to_string().contains(value));
            values.values_mut().for_each(Zeroize::zeroize);
        }
        fs::write(fixture.directory.join("client-secret"), "A".repeat(2048))?;
        assert!(BffConfig::from_values(&fixture.values).is_err());
        Ok(())
    }
    #[test]
    fn production_requires_https_host_cookies_and_restricted_secret_files()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut fixture = Fixture::new()?;
        fixture.production();
        assert!(BffConfig::from_values(&fixture.values).is_ok());
        for (field, value) in [
            ("BFF_PUBLIC_ORIGIN", "http://localhost:5174"),
            ("ISSUER", "http://localhost:5173"),
            ("BFF_COOKIE_NAME", "demo-a-session"),
            ("BFF_ISSUER_CONNECT_HOST", "identity-web"),
        ] {
            let mut values = fixture.values.clone();
            values.insert(field.into(), value.into());
            assert!(BffConfig::from_values(&values).is_err());
            values.values_mut().for_each(Zeroize::zeroize);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for name in ["client-secret", "keys.json"] {
                let path = fixture.directory.join(name);
                fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
                assert!(BffConfig::from_values(&fixture.values).is_err());
                fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
            }
        }
        Ok(())
    }
}
