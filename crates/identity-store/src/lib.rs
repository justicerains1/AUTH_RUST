//! PostgreSQL and Redis connectivity. Repository transactions are implemented in T03.
pub mod accounts;
pub mod migrations;
pub mod repository;
pub mod security;
pub mod sessions;
use identity_core::config::Config;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;

#[derive(Clone)]
pub struct Dependencies {
    pub postgres: sqlx::PgPool,
    redis: redis::Client,
}

#[derive(Debug)]
pub struct DependencyUnavailable;

impl Dependencies {
    // Lazy connections let liveness remain available during a dependency outage.
    pub fn new(config: &Config) -> Result<Self, DependencyUnavailable> {
        let postgres = PgPoolOptions::new()
            .max_connections(config.database_pool_max)
            .acquire_timeout(Duration::from_secs(2))
            .connect_lazy(config.database_url.expose())
            .map_err(|_| DependencyUnavailable)?;
        let redis =
            redis::Client::open(config.redis_url.expose()).map_err(|_| DependencyUnavailable)?;
        Ok(Self { postgres, redis })
    }

    pub async fn ready(&self) -> bool {
        let (postgres, redis) = tokio::join!(
            tokio::time::timeout(Duration::from_secs(2), async {
                sqlx::query("SELECT 1").execute(&self.postgres).await
            }),
            tokio::time::timeout(Duration::from_secs(2), async {
                let mut connection = self.redis.get_multiplexed_async_connection().await?;
                redis::cmd("PING")
                    .query_async::<String>(&mut connection)
                    .await
            })
        );
        matches!(postgres, Ok(Ok(_))) && matches!(redis, Ok(Ok(ref response)) if response == "PONG")
    }

    pub async fn close(&self) {
        self.postgres.close().await;
    }
}
