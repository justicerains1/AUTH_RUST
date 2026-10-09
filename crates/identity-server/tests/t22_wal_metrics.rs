//! Test-only actual PostgreSQL archiver/Worker metric agreement; dedicated instance required.
use identity_core::{clock::SystemClock, config::Config};
use identity_store::migrations::{MigrationTarget, migrate};
use sqlx::{Row, postgres::PgPoolOptions};
use std::{collections::BTreeMap, error::Error, sync::Arc};
use uuid::Uuid;
#[tokio::test]
async fn t22_actual_wal_metrics() -> Result<(), Box<dyn Error + Send + Sync>> {
    if std::env::var("T22_WAL_DEDICATED_INSTANCE").as_deref() != Ok("1") {
        return Err("explicit dedicated WAL instance required".into());
    }
    let database = std::env::var("TEST_DATABASE_URL")?;
    MigrationTarget::from_environment("test", &database, false, true)?;
    let mut values = BTreeMap::new();
    for (key, value) in [
        ("APP_ENV", "test"),
        ("BIND", "127.0.0.1:0"),
        ("ISSUER", "http://localhost:5173"),
        ("RP_ID", "localhost"),
        ("SIGNING_KID", "wal-fixture"),
        ("SMTP_HOST", "127.0.0.1"),
        ("SMTP_PORT", "1025"),
        ("SMTP_FROM", "no-reply@localhost"),
        ("SMTP_TLS", "disabled"),
    ] {
        values.insert(key.to_owned(), value.to_owned());
    }
    values.insert("DATABASE_URL".into(), database.clone());
    values.insert("REDIS_URL".into(), "redis://127.0.0.1:6379".into());
    for field in [
        "SIGNING_KEY_FILE",
        "ENCRYPTION_KEYS_FILE",
        "ACTIVE_ENCRYPTION_KID",
    ] {
        values.insert(field.to_owned(), std::env::var(field)?);
    }
    let config = Config::from_values(&values)?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database)
        .await?;
    migrate(&pool).await?;
    let phase = std::env::var("T22_WAL_EXPECTED_PHASE")?;
    if phase == "permission-failure" {
        let role = format!("wal_metrics_{}", Uuid::new_v4().simple());
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE ROLE {role}")))
            .execute(&pool)
            .await?;
        for statement in [
            format!("GRANT USAGE ON SCHEMA public TO {role}"),
            format!("GRANT SELECT ON email_outbox TO {role}"),
        ] {
            sqlx::query(sqlx::AssertSqlSafe(statement))
                .execute(&pool)
                .await?;
        }
        sqlx::query("REVOKE SELECT ON pg_catalog.pg_stat_archiver FROM PUBLIC")
            .execute(&pool)
            .await?;
        let role_copy = role.clone();
        let denied = PgPoolOptions::new()
            .max_connections(1)
            .after_connect(move |connection, _| {
                let role = role_copy.clone();
                Box::pin(async move {
                    sqlx::query(sqlx::AssertSqlSafe(format!("SET ROLE {role}")))
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .connect(&database)
            .await?;
        let worker = identity_worker::outbox::MailWorker::new(
            &config,
            denied.clone(),
            Arc::new(SystemClock),
        )?;
        assert!(worker.operational_metrics().await.is_err());
        denied.close().await;
        sqlx::query("GRANT SELECT ON pg_catalog.pg_stat_archiver TO PUBLIC")
            .execute(&pool)
            .await?;
        for statement in [format!("DROP OWNED BY {role}"), format!("DROP ROLE {role}")] {
            sqlx::query(sqlx::AssertSqlSafe(statement))
                .execute(&pool)
                .await?;
        }
    } else {
        let worker =
            identity_worker::outbox::MailWorker::new(&config, pool.clone(), Arc::new(SystemClock))?;
        let text = worker.operational_metrics().await?;
        let snapshot = sqlx::query("SELECT archived_count::bigint,failed_count::bigint,EXTRACT(EPOCH FROM last_archived_time)::float8 AS archived,EXTRACT(EPOCH FROM last_failed_time)::float8 AS failed FROM pg_stat_archiver").fetch_one(&pool).await?;
        let metric = |name: &str| -> Result<f64, Box<dyn Error + Send + Sync>> {
            let value = text
                .lines()
                .find_map(|line| line.strip_prefix(&format!("{name} ")))
                .ok_or("actual metric missing")?;
            Ok(value.parse()?)
        };
        assert_eq!(metric("identity_postgres_wal_archive_enabled")?, 1.0);
        assert_eq!(
            metric("identity_postgres_wal_archived_total")?,
            snapshot.try_get::<i64, _>("archived_count")? as f64
        );
        assert_eq!(
            metric("identity_postgres_wal_archive_failures_total")?,
            snapshot.try_get::<i64, _>("failed_count")? as f64
        );
        assert_eq!(
            metric("identity_postgres_wal_last_archived_timestamp_seconds")?,
            snapshot
                .try_get::<Option<f64>, _>("archived")?
                .unwrap_or(0.0)
        );
        assert_eq!(
            metric("identity_postgres_wal_last_failed_timestamp_seconds")?,
            snapshot.try_get::<Option<f64>, _>("failed")?.unwrap_or(0.0)
        );
        if phase == "failed" {
            assert!(metric("identity_postgres_wal_archive_failures_total")? > 0.0);
            assert!(
                metric("identity_postgres_wal_last_failed_timestamp_seconds")?
                    > metric("identity_postgres_wal_last_archived_timestamp_seconds")?
            );
        }
        if phase == "recovered" {
            assert!(
                metric("identity_postgres_wal_last_archived_timestamp_seconds")?
                    >= metric("identity_postgres_wal_last_failed_timestamp_seconds")?
            );
            assert!(metric("identity_postgres_wal_archived_total")? > 0.0);
        }
        assert!(!text.contains("wal_name"));
        assert!(!text.contains("last_archived_wal"));
        assert!(!text.contains("last_failed_wal"));
        pool.close().await;
        assert!(worker.operational_metrics().await.is_err());
    }
    pool.close().await;
    println!(
        "PASS actual WAL Worker metric phase and PostgreSQL snapshot agree; unavailable queries do not return healthy zeroes"
    );
    Ok(())
}
