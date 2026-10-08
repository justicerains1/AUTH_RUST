//! Dedicated local full-identity recovery harness. Real ceremonies run in Chromium externally.
use axum::{Json, Router, extract::Request, routing::get};
use identity_core::{
    clock::Clock,
    config::Config,
    oauth::Scope,
    security::{Password, PasswordService},
};
use identity_server::accounts::{AuthAppState, accounts_router};
use identity_store::{
    Dependencies,
    migrations::{MigrationTarget, migrate},
    oauth::{NewClient, OAuthStore},
};
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use std::{
    collections::BTreeMap,
    error::Error,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
};
use time::OffsetDateTime;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
#[derive(Clone)]
struct RestoreClock {
    seconds: Arc<AtomicI64>,
    path: PathBuf,
}
impl Clock for RestoreClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(self.seconds.load(Ordering::SeqCst))
            .unwrap_or(OffsetDateTime::UNIX_EPOCH)
    }
}
fn required(name: &str) -> TestResult<String> {
    std::env::var(name).map_err(|_| format!("missing restore test field: {name}").into())
}
fn private_json(path: &std::path::Path, value: &Value) -> TestResult {
    std::fs::write(path, serde_json::to_vec(value)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
#[tokio::test]
async fn t22_identity_restore_harness() -> TestResult {
    let directory = PathBuf::from(required("T22_RESTORE_DIRECTORY")?);
    let issuer = required("T22_RESTORE_ISSUER")?;
    let database = required("TEST_DATABASE_URL")?;
    let target = MigrationTarget::from_environment(&required("APP_ENV")?, &database, false, true)?;
    let mut values: BTreeMap<String, String> = [
        ("APP_ENV", "test"),
        ("BIND", "127.0.0.1:0"),
        ("ISSUER", issuer.as_str()),
        ("RP_ID", "localhost"),
        ("SIGNING_KID", "restore-signing"),
        ("SMTP_HOST", "127.0.0.1"),
        ("SMTP_PORT", "1025"),
        ("SMTP_FROM", "no-reply@localhost"),
        ("SMTP_TLS", "disabled"),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.into()))
    .collect();
    for (name, value) in [
        ("DATABASE_URL", database),
        ("REDIS_URL", required("REDIS_URL")?),
        ("SIGNING_KEY_FILE", required("T22_RESTORE_SIGNING_FILE")?),
        ("ENCRYPTION_KEYS_FILE", required("T22_RESTORE_KEYS_FILE")?),
        ("ACTIVE_ENCRYPTION_KID", required("T22_RESTORE_ACTIVE_KID")?),
    ] {
        values.insert(name.into(), value);
    }
    let config = Config::from_values(&values)?;
    let recovered = required("T22_RESTORE_MODE")? == "recovered";
    let manifest_path = directory.join("scenario.json");
    let mut scenario: Value = if recovered {
        serde_json::from_slice(&std::fs::read(&manifest_path)?)?
    } else {
        json!({"schema":format!("identity_test_restore_{}",Uuid::new_v4().simple()),"email":format!("recovery-{}@example.test",Uuid::new_v4().simple())})
    };
    let schema = scenario["schema"].as_str().ok_or("schema missing")?;
    let admin = target.connect().await?;
    if !recovered {
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&admin)
            .await?;
    }
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect_with(target.test_schema_options(schema)?)
        .await?;
    if !recovered {
        migrate(&pool).await?;
    }
    let clock_path = directory.join("clock.json");
    let seconds = if recovered {
        serde_json::from_slice::<Value>(&std::fs::read(&clock_path)?)?["seconds"]
            .as_i64()
            .ok_or("clock missing")?
            + 30
    } else {
        OffsetDateTime::now_utc().unix_timestamp()
    };
    let clock = RestoreClock {
        seconds: Arc::new(AtomicI64::new(seconds)),
        path: clock_path,
    };
    if !recovered {
        let hash = PasswordService::initialize(4)
            .await?
            .hash(&Password::new(&required("T22_RESTORE_PASSWORD")?)?)
            .await?;
        let user = Uuid::new_v4();
        sqlx::query("INSERT INTO users(id,email,password_hash,verified,status) VALUES($1,$2,$3,true,'active')").bind(user).bind(scenario["email"].as_str().ok_or("email missing")?).bind(hash.as_str()).execute(&pool).await?;
        scenario["user_id"] = json!(user);
        let store = OAuthStore::new(pool.clone(), Arc::new(clock.clone()));
        let mut clients = Vec::new();
        for suffix in ["live", "revoked"] {
            let client = store
                .create_client(&NewClient {
                    client_id: format!("restore-{suffix}"),
                    name: format!("Local recovery {suffix}"),
                    allowed_scopes: vec![Scope::OpenId, Scope::Email],
                    redirect_uris: vec![format!("{issuer}/callback")],
                    logout_uris: vec![],
                    production: false,
                })
                .await?;
            clients
                .push(json!({"client_id":client.client_id,"secret":client.client_secret.expose()}));
        }
        scenario["clients"] = json!(clients);
        private_json(&manifest_path, &scenario)?;
    } else {
        let factors: i64 = sqlx::query_scalar("SELECT count(*) FROM totp_factors WHERE confirmed")
            .fetch_one(&pool)
            .await?;
        let passkeys: i64 = sqlx::query_scalar("SELECT count(*) FROM webauthn_credentials")
            .fetch_one(&pool)
            .await?;
        assert_eq!(factors, 1);
        assert_eq!(passkeys, 1);
    }
    let mut deps = Dependencies::new(&config).map_err(|_| "restore dependencies invalid")?;
    deps.postgres = pool.clone();
    let state = AuthAppState::new_with_clock(&config, deps, Arc::new(clock.clone())).await?;
    let control_key = required("T22_RESTORE_CONTROL_KEY")?;
    let control_pool = pool.clone();
    let control = Router::new().route(
        "/__restore/clock",
        get(move |request: Request| {
            let clock = clock.clone();
            let key = control_key.clone();
            let pool = control_pool.clone();
            async move {
                if request
                    .headers()
                    .get("x-test-key")
                    .and_then(|x| x.to_str().ok())
                    != Some(key.as_str())
                {
                    return (
                        axum::http::StatusCode::FORBIDDEN,
                        Json(json!({"error":"forbidden"})),
                    );
                }
                if request.uri().query() == Some("advance=1") {
                    clock.seconds.fetch_add(30, Ordering::SeqCst);
                }
                let seconds = clock.seconds.load(Ordering::SeqCst);
                if private_json(&clock.path, &json!({"seconds":seconds})).is_err() {
                    return (
                        axum::http::StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error":"clock"})),
                    );
                }
                let factors: i64 =
                    sqlx::query_scalar("SELECT count(*) FROM totp_factors WHERE confirmed")
                        .fetch_one(&pool)
                        .await
                        .unwrap_or(-1);
                let passkeys: i64 = sqlx::query_scalar("SELECT count(*) FROM webauthn_credentials")
                    .fetch_one(&pool)
                    .await
                    .unwrap_or(-1);
                (
                    axum::http::StatusCode::OK,
                    Json(json!({"seconds":seconds,"factors":factors,"passkeys":passkeys})),
                )
            }
        }),
    );
    let app=accounts_router(state,control.route("/",get(||async{axum::response::Html("<!doctype html><html><head><title>Local recovery ceremony</title></head><body><main>Local recovery ceremony</main></body></html>")})));
    let port = url::Url::parse(&issuer)?
        .port()
        .ok_or("local explicit port missing")?;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    let server = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });
    println!("T22_IDENTITY_RESTORE_READY");
    tokio::task::spawn_blocking(|| {
        use std::io::BufRead;
        let mut input = String::new();
        let _ = std::io::stdin().lock().read_line(&mut input);
    })
    .await?;
    server.abort();
    let _ = server.await;
    pool.close().await;
    admin.close().await;
    println!(
        "PASS T22 identity restore harness used explicit test-only database and closed its own API"
    );
    Ok(())
}
