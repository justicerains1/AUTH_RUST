//! Explicit migration targets. Errors never include connection strings or driver diagnostics.
use identity_core::config::Environment;
use sqlx::{ConnectOptions, PgPool, migrate::Migrator, postgres::PgConnectOptions};
use std::{fmt, str::FromStr, time::Duration};
use url::Url;

pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

pub struct MigrationTarget {
    pub environment: Environment,
    database_name: String,
    connect_options: PgConnectOptions,
    test_target: bool,
}

impl fmt::Debug for MigrationTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MigrationTarget")
            .field("environment", &self.environment)
            .field("database_name", &self.database_name)
            .field("test_target", &self.test_target)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationError {
    InvalidEnvironment,
    InvalidDatabaseUrl,
    ProductionAcknowledgementRequired,
    TestTargetRejected,
    InvalidTestSchema,
    ConnectionUnavailable,
    ApplyFailed,
}

impl fmt::Display for MigrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidEnvironment => "invalid or missing APP_ENV",
            Self::InvalidDatabaseUrl => "invalid or missing DATABASE_URL",
            Self::ProductionAcknowledgementRequired => {
                "production migration requires --allow-production"
            }
            Self::TestTargetRejected => "test target requires APP_ENV=test and database identity_test",
            Self::InvalidTestSchema => "invalid test schema; expected identity_test_ and lowercase letters, digits or underscores",
            Self::ConnectionUnavailable => "migration database connection unavailable",
            Self::ApplyFailed => "migration failed; inspect controlled database diagnostics",
        })
    }
}

impl std::error::Error for MigrationError {}

impl MigrationTarget {
    /// Validate the effective driver database, including URL query overrides, before connecting.
    pub fn from_environment(
        app_env: &str,
        database_url: &str,
        allow_production: bool,
        test_target: bool,
    ) -> Result<Self, MigrationError> {
        let environment = match app_env {
            "development" => Environment::Development,
            "test" => Environment::Test,
            "production" => Environment::Production,
            _ => return Err(MigrationError::InvalidEnvironment),
        };
        if test_target && environment != Environment::Test {
            return Err(MigrationError::TestTargetRejected);
        }
        if environment == Environment::Production && !allow_production {
            return Err(MigrationError::ProductionAcknowledgementRequired);
        }
        let url = Url::parse(database_url).map_err(|_| MigrationError::InvalidDatabaseUrl)?;
        if !matches!(url.scheme(), "postgres" | "postgresql")
            || url.host_str().is_none()
            || url.username().is_empty()
            || url.path().len() <= 1
            || url.path().trim_start_matches('/').contains('/')
            || url.fragment().is_some()
        {
            return Err(MigrationError::InvalidDatabaseUrl);
        }
        // SQLx logs unknown query parameters; reject them before handing the URL to the driver.
        // Arbitrary `options` could redirect search_path into another schema, so they are not accepted.
        for (key, _) in url.query_pairs() {
            if !matches!(
                key.as_ref(),
                "sslmode"
                    | "ssl-mode"
                    | "sslrootcert"
                    | "ssl-root-cert"
                    | "ssl-ca"
                    | "sslcert"
                    | "ssl-cert"
                    | "sslkey"
                    | "ssl-key"
                    | "statement-cache-capacity"
                    | "host"
                    | "hostaddr"
                    | "port"
                    | "dbname"
                    | "user"
                    | "password"
                    | "application_name"
            ) {
                return Err(MigrationError::InvalidDatabaseUrl);
            }
        }
        let connect_options = PgConnectOptions::from_str(database_url)
            .map_err(|_| MigrationError::InvalidDatabaseUrl)?
            .disable_statement_logging();
        let database_name = connect_options
            .get_database()
            .filter(|name| {
                !name.is_empty()
                    && name.len() <= 63
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            })
            .ok_or(MigrationError::InvalidDatabaseUrl)?
            .to_owned();
        if (environment == Environment::Test || test_target) && database_name != "identity_test" {
            return Err(MigrationError::TestTargetRejected);
        }
        Ok(Self {
            environment,
            database_name,
            connect_options,
            test_target,
        })
    }

    pub fn database_name(&self) -> &str {
        &self.database_name
    }

    /// Test schemas are fixed-prefix identifiers, validated independently of SQL parameters.
    pub fn test_schema_options(&self, schema: &str) -> Result<PgConnectOptions, MigrationError> {
        if self.environment != Environment::Test || self.database_name != "identity_test" {
            return Err(MigrationError::TestTargetRejected);
        }
        if !schema.starts_with("identity_test_")
            || schema.len() <= "identity_test_".len()
            || schema.len() > 63
            || !schema
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(MigrationError::InvalidTestSchema);
        }
        Ok(self
            .connect_options
            .clone()
            .options([("search_path", schema), ("timezone", "UTC")]))
    }

    pub async fn connect(&self) -> Result<PgPool, MigrationError> {
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .acquire_timeout(Duration::from_secs(5))
            .connect_with(self.connect_options.clone().options([("timezone", "UTC")]))
            .await
            .map_err(|_| MigrationError::ConnectionUnavailable)
    }
}

/// SQLx records checksums and acquires its advisory migration lock; no destructive down is supplied.
pub async fn migrate(pool: &PgPool) -> Result<(), MigrationError> {
    MIGRATOR
        .run(pool)
        .await
        .map_err(|_| MigrationError::ApplyFailed)
}

#[cfg(test)]
mod tests {
    use super::{MigrationError, MigrationTarget};

    #[test]
    fn production_migration_requires_explicit_acknowledgement_but_is_supported() {
        let url = "postgres://identity:REDACTION_SENTINEL@localhost/identity_production";
        assert!(matches!(
            MigrationTarget::from_environment("production", url, false, false),
            Err(MigrationError::ProductionAcknowledgementRequired)
        ));
        assert!(MigrationTarget::from_environment("production", url, true, false).is_ok());
        assert!(matches!(
            MigrationTarget::from_environment("production", url, true, true),
            Err(MigrationError::TestTargetRejected)
        ));
    }

    #[test]
    fn test_database_uses_effective_driver_database_and_rejects_overrides() {
        for url in [
            "postgres://identity@localhost/identity_development",
            "postgres://identity@localhost/identity_test?dbname=identity_production",
            "postgres://identity@localhost/identity_test?options=-csearch_path%3Dpublic",
            "postgres://identity@localhost/identity_test?unknown=REDACTION_SENTINEL",
        ] {
            assert!(MigrationTarget::from_environment("test", url, false, true).is_err());
        }
        assert!(
            MigrationTarget::from_environment(
                "test",
                "postgres://identity@localhost/identity_test?sslmode=disable",
                false,
                true,
            )
            .is_ok()
        );
    }

    #[test]
    fn test_schema_is_scoped_and_never_interprets_arbitrary_sql() {
        let target = MigrationTarget::from_environment(
            "test",
            "postgres://identity@localhost/identity_test",
            false,
            true,
        );
        assert!(target.is_ok());
        if let Ok(target) = target {
            assert!(target.test_schema_options("identity_test_t03_123").is_ok());
            for schema in [
                "public",
                "identity_test_",
                "identity_test_x;DROP SCHEMA public",
                "identity_test_X",
            ] {
                assert!(target.test_schema_options(schema).is_err());
            }
        }
    }

    #[test]
    fn errors_and_debug_do_not_echo_secret_connection_values() {
        let target = MigrationTarget::from_environment(
            "development",
            "postgres://identity:REDACTION_SENTINEL@localhost/identity_development",
            false,
            false,
        );
        assert!(target.is_ok());
        if let Ok(target) = target {
            assert!(!format!("{target:?}").contains("REDACTION_SENTINEL"));
        }
        assert!(
            !MigrationError::InvalidDatabaseUrl
                .to_string()
                .contains("REDACTION_SENTINEL")
        );
    }
}
