//! Isolated T19 product acceptance with actual authentication and PostgreSQL transactions.
//! Private controls expose only fixed fixture material and never replace production API responses.
use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    routing::{get, post},
};
use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use identity_core::{
    clock::SystemClock,
    config::Config,
    mfa::{RecoveryCodes, TotpSecret},
    security::{AeadKeyRing, Password, PasswordService, constant_time_equal, token_digest},
};
use identity_server::accounts::{AuthAppState, accounts_router};
use identity_store::{
    Dependencies,
    migrations::{MigrationTarget, migrate},
    oauth::{NewClient, OAuthStore},
};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Transaction, postgres::PgPoolOptions};
use std::{collections::BTreeMap, error::Error, net::SocketAddr, sync::Arc};
use tokio::sync::Mutex;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
type ControlResponse = (StatusCode, Json<Value>);
const SCENARIOS: [&str; 4] = ["users", "clients", "members", "pending"];
const PENDING_QUERY: &str = "scenario=pending&operation=disable-client";

fn configuration() -> TestResult<Config> {
    let mut values = BTreeMap::new();
    for (key, value) in [
        ("APP_ENV", "test"),
        ("BIND", "127.0.0.1:0"),
        ("ISSUER", "http://localhost:5340"),
        ("RP_ID", "localhost"),
        ("SIGNING_KID", "local-signing-1"),
        ("SMTP_HOST", "127.0.0.1"),
        ("SMTP_PORT", "1025"),
        ("SMTP_FROM", "no-reply@localhost"),
        ("SMTP_TLS", "disabled"),
    ] {
        values.insert(key.to_owned(), value.to_owned());
    }
    for field in ["DATABASE_URL", "REDIS_URL"] {
        values.insert(
            field.to_owned(),
            std::env::var(field).map_err(|_| "explicit test database/Redis required")?,
        );
    }
    for (key, field) in [
        ("SIGNING_KEY_FILE", "ADMIN_MANAGEMENT_SIGNING_KEY_FILE"),
        (
            "ENCRYPTION_KEYS_FILE",
            "ADMIN_MANAGEMENT_ENCRYPTION_KEYS_FILE",
        ),
        (
            "ACTIVE_ENCRYPTION_KID",
            "ADMIN_MANAGEMENT_ACTIVE_ENCRYPTION_KID",
        ),
    ] {
        values.insert(
            key.to_owned(),
            std::env::var(field).map_err(|_| "private test configuration required")?,
        );
    }
    let environment = std::env::var("APP_ENV").map_err(|_| "APP_ENV=test required")?;
    let database = std::env::var("TEST_DATABASE_URL")
        .map_err(|_| "TEST_DATABASE_URL identity_test required")?;
    MigrationTarget::from_environment(&environment, &database, false, true)?;
    Ok(Config::from_values(&values)?)
}

async fn isolated(config: &Config) -> TestResult<(PgPool, PgPool, String)> {
    let target =
        MigrationTarget::from_environment("test", config.database_url.expose(), false, true)?;
    let schema = format!("identity_test_t19m_{}", Uuid::new_v4().simple());
    let options = target
        .test_schema_options(&schema)?
        .application_name(&schema);
    let admin = target.connect().await?;
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await?;
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect_with(options)
        .await?;
    if let Err(error) = migrate(&pool).await {
        cleanup(admin, pool, schema).await?;
        return Err(error.into());
    }
    Ok((admin, pool, schema))
}

async fn cleanup(admin: PgPool, pool: PgPool, schema: String) -> TestResult {
    pool.close().await;
    let result = sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin)
        .await;
    admin.close().await;
    result.map_err(|_| "test schema cleanup failed")?;
    Ok(())
}

struct FixtureUser {
    id: Uuid,
    recoveries: Vec<String>,
}

async fn fixture_user(
    pool: &PgPool,
    ring: &AeadKeyRing,
    email: &str,
    hash: &str,
    with_factor: bool,
    admin: bool,
) -> TestResult<FixtureUser> {
    let user = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,email,password_hash,verified) VALUES($1,$2,$3,true)")
        .bind(user)
        .bind(email)
        .bind(hash)
        .execute(pool)
        .await?;
    let mut recoveries = Vec::new();
    if with_factor {
        let secret = TotpSecret::generate()?;
        let encrypted = ring.encrypt(user, "totp-seed", secret.bytes())?;
        sqlx::query("INSERT INTO totp_factors(id,user_id,encrypted_seed,encryption_kid,encryption_nonce,confirmed) VALUES($1,$2,$3,$4,$5,true)")
            .bind(Uuid::new_v4()).bind(user)
            .bind(BASE64_URL_SAFE_NO_PAD.decode(&encrypted.ciphertext)?)
            .bind(&encrypted.kid).bind(BASE64_URL_SAFE_NO_PAD.decode(&encrypted.nonce)?)
            .execute(pool).await?;
        let recovery = RecoveryCodes::generate()?;
        recoveries = recovery.expose().to_vec();
        if recoveries.len() != 10 {
            return Err("standard recovery fixture must contain ten codes".into());
        }
        for code in &recoveries {
            sqlx::query("INSERT INTO recovery_codes(id,user_id,code_hash) VALUES($1,$2,$3)")
                .bind(Uuid::new_v4())
                .bind(user)
                .bind(token_digest(code).as_slice())
                .execute(pool)
                .await?;
        }
    }
    if admin {
        sqlx::query("INSERT INTO admin_memberships(id,user_id,enabled) VALUES($1,$2,true)")
            .bind(Uuid::new_v4())
            .bind(user)
            .execute(pool)
            .await?;
    }
    Ok(FixtureUser {
        id: user,
        recoveries,
    })
}

struct HeldClient {
    transaction: Transaction<'static, Postgres>,
    backend_pid: i32,
}

#[derive(Clone)]
struct ControlState {
    pool: PgPool,
    key: Arc<String>,
    admins: Arc<BTreeMap<&'static str, FixtureUser>>,
    target_id: Uuid,
    member: Arc<FixtureUser>,
    client_id: Uuid,
    pending_client_id: Uuid,
    application_name: Arc<String>,
    held: Arc<Mutex<Option<HeldClient>>>,
}

fn control_error(status: StatusCode, error: &str) -> ControlResponse {
    (status, Json(json!({"error":error})))
}

fn permitted(state: &ControlState, request: &Request) -> bool {
    request
        .headers()
        .get("x-test-key")
        .is_some_and(|key| constant_time_equal(key.as_bytes(), state.key.as_bytes()))
}

fn scenario(request: &Request) -> Option<&'static str> {
    match request.uri().query()? {
        "scenario=users" => Some("users"),
        "scenario=clients" => Some("clients"),
        "scenario=members" => Some("members"),
        "scenario=pending" => Some("pending"),
        _ => None,
    }
}

async fn unused_recoveries(pool: &PgPool, user: &FixtureUser) -> Result<Vec<String>, sqlx::Error> {
    let hashes: Vec<Vec<u8>> = sqlx::query_scalar(
        "SELECT code_hash FROM recovery_codes WHERE user_id=$1 AND consumed_at IS NULL",
    )
    .bind(user.id)
    .fetch_all(pool)
    .await?;
    Ok(user
        .recoveries
        .iter()
        .filter(|code| {
            let digest = token_digest(code);
            hashes
                .iter()
                .any(|hash| constant_time_equal(hash, digest.as_slice()))
        })
        .cloned()
        .collect())
}

async fn material(State(state): State<ControlState>, request: Request) -> ControlResponse {
    if !permitted(&state, &request) {
        return control_error(StatusCode::FORBIDDEN, "forbidden");
    }
    let Some(scenario) = scenario(&request) else {
        return control_error(StatusCode::BAD_REQUEST, "fixed scenario required");
    };
    let Some(actor) = state.admins.get(scenario) else {
        return control_error(StatusCode::INTERNAL_SERVER_ERROR, "actor fixture missing");
    };
    let Ok(recoveries) = unused_recoveries(&state.pool, actor).await else {
        return control_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "fixture lookup unavailable",
        );
    };
    let mut output = json!({
        "actorId":actor.id,
        "recoveries":recoveries,
        "targetId":state.target_id,
        "memberId":state.member.id,
        "clientDbId":if scenario == "pending" { state.pending_client_id } else { state.client_id },
    });
    if scenario == "members" {
        let Ok(codes) = unused_recoveries(&state.pool, &state.member).await else {
            return control_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "fixture lookup unavailable",
            );
        };
        output["memberRecoveries"] = json!(codes);
    }
    (StatusCode::OK, Json(output))
}

fn fixed_pending(state: &ControlState, request: &Request) -> Option<ControlResponse> {
    if !permitted(state, request) {
        Some(control_error(StatusCode::FORBIDDEN, "forbidden"))
    } else if request.uri().query() != Some(PENDING_QUERY) {
        Some(control_error(
            StatusCode::BAD_REQUEST,
            "fixed pending operation required",
        ))
    } else {
        None
    }
}

async fn hold(State(state): State<ControlState>, request: Request) -> ControlResponse {
    if let Some(error) = fixed_pending(&state, &request) {
        return error;
    }
    let mut held = state.held.lock().await;
    if held.is_some() {
        return control_error(StatusCode::CONFLICT, "fixture lock already held");
    }
    let result: Result<HeldClient, sqlx::Error> = async {
        let mut transaction = state.pool.begin().await?;
        let backend_pid = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *transaction)
            .await?;
        // The real update_client transaction takes this row lock after its admin/user locks.
        // Do not take those earlier locks here: pending must wait on the intended client row.
        sqlx::query("SELECT id FROM oauth_clients WHERE id=$1 FOR UPDATE")
            .bind(state.pending_client_id)
            .fetch_one(&mut *transaction)
            .await?;
        Ok(HeldClient {
            transaction,
            backend_pid,
        })
    }
    .await;
    match result {
        Ok(lock) => {
            *held = Some(lock);
            (StatusCode::OK, Json(json!({"held":true})))
        }
        Err(_) => control_error(StatusCode::SERVICE_UNAVAILABLE, "fixture lock unavailable"),
    }
}

async fn release(State(state): State<ControlState>, request: Request) -> ControlResponse {
    if let Some(error) = fixed_pending(&state, &request) {
        return error;
    }
    let mut held = state.held.lock().await;
    let Some(lock) = held.take() else {
        return (StatusCode::OK, Json(json!({"held":false,"released":false})));
    };
    match lock.transaction.rollback().await {
        Ok(()) => (StatusCode::OK, Json(json!({"held":false,"released":true}))),
        Err(_) => control_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "fixture release unavailable",
        ),
    }
}

async fn held_status(State(state): State<ControlState>, request: Request) -> ControlResponse {
    if let Some(error) = fixed_pending(&state, &request) {
        return error;
    }
    let backend_pid = state
        .held
        .lock()
        .await
        .as_ref()
        .map(|lock| lock.backend_pid);
    let result: Result<(i64, bool, i64), sqlx::Error> = async {
        let blocked = if let Some(pid) = backend_pid {
            sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND application_name=$2 AND wait_event_type='Lock' AND $1=ANY(pg_blocking_pids(pid))")
                .bind(pid).bind(state.application_name.as_str()).fetch_one(&state.pool).await?
        } else {
            0
        };
        let (enabled, updates): (bool, i64) = sqlx::query_as("SELECT enabled,(SELECT count(*) FROM audit_events WHERE event='oauth.client_updated' AND target_id=$1) FROM oauth_clients WHERE id=$1")
            .bind(state.pending_client_id).fetch_one(&state.pool).await?;
        Ok((blocked, enabled, updates))
    }
    .await;
    match result {
        Ok((blocked, enabled, updates)) => (
            StatusCode::OK,
            Json(
                json!({"held":backend_pid.is_some(),"blockedMutations":blocked,"enabled":enabled,"clientUpdateCount":updates}),
            ),
        ),
        Err(_) => control_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "fixture lock lookup unavailable",
        ),
    }
}

async fn last_admin(State(state): State<ControlState>, request: Request) -> ControlResponse {
    if !permitted(&state, &request) {
        return control_error(StatusCode::FORBIDDEN, "forbidden");
    }
    if request.uri().query() != Some("scenario=members") {
        return control_error(StatusCode::BAD_REQUEST, "fixed members scenario required");
    }
    let Some(actor) = state.admins.get("members") else {
        return control_error(StatusCode::INTERNAL_SERVER_ERROR, "actor fixture missing");
    };
    let restore = request.uri().path() == "/__test/restore-admins";
    let ids: Vec<Uuid> = state.admins.values().map(|admin| admin.id).collect();
    let result: Result<bool, sqlx::Error> = async {
        let mut transaction = state.pool.begin().await?;
        if !restore {
            let candidate_enabled: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM admin_memberships WHERE user_id=$1 AND enabled)",
            )
            .bind(state.member.id)
            .fetch_one(&mut *transaction)
            .await?;
            if candidate_enabled {
                return Ok(false);
            }
        }
        let result = sqlx::query(
            "UPDATE admin_memberships SET enabled=($2 OR user_id=$3) WHERE user_id=ANY($1::uuid[])",
        )
        .bind(&ids)
        .bind(restore)
        .bind(actor.id)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != SCENARIOS.len() as u64 {
            return Err(sqlx::Error::RowNotFound);
        }
        transaction.commit().await?;
        Ok(true)
    }
    .await;
    match result {
        Ok(true) => (
            StatusCode::OK,
            Json(json!({"lastAdmin":!restore,"restored":restore})),
        ),
        Ok(false) => control_error(
            StatusCode::CONFLICT,
            "remove fixed candidate membership first",
        ),
        Err(_) => control_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "fixture membership setup unavailable",
        ),
    }
}

async fn run_harness(config: &Config, pool: &PgPool, schema: &str) -> TestResult {
    let password =
        std::env::var("ADMIN_MANAGEMENT_PASSWORD").map_err(|_| "private test password required")?;
    let key = std::env::var("ADMIN_MANAGEMENT_CONTROL_KEY")
        .map_err(|_| "private test control key required")?;
    if key.len() < 32 {
        return Err("private control key must contain at least 32 bytes".into());
    }
    let service = PasswordService::initialize(config.argon2_parallelism_limit).await?;
    let hash = service.hash(&Password::new(&password)?).await?;
    let ring = AeadKeyRing::load_file(&config.encryption_keys_file, &config.active_encryption_kid)?;
    let mut admins = BTreeMap::new();
    for scenario in SCENARIOS {
        let email = format!("management-admin-{scenario}@example.test");
        admins.insert(
            scenario,
            fixture_user(pool, &ring, &email, hash.as_str(), true, true).await?,
        );
    }
    let target = fixture_user(
        pool,
        &ring,
        "management-target@example.test",
        hash.as_str(),
        false,
        false,
    )
    .await?;
    let member = fixture_user(
        pool,
        &ring,
        "management-member@example.test",
        hash.as_str(),
        true,
        false,
    )
    .await?;
    let oauth = OAuthStore::new(pool.clone(), Arc::new(SystemClock));
    let mut clients = Vec::new();
    for client_id in ["management-client", "management-pending-client"] {
        let client = oauth
            .create_client(&NewClient {
                client_id: client_id.into(),
                name: client_id.into(),
                allowed_scopes: vec![identity_core::oauth::Scope::OpenId],
                redirect_uris: vec!["http://localhost:5340/callback".into()],
                logout_uris: vec![],
                production: false,
            })
            .await?;
        clients.push(client.id);
    }
    let state = ControlState {
        pool: pool.clone(),
        key: Arc::new(key),
        admins: Arc::new(admins),
        target_id: target.id,
        member: Arc::new(member),
        client_id: clients[0],
        pending_client_id: clients[1],
        application_name: Arc::new(schema.to_owned()),
        held: Arc::new(Mutex::new(None)),
    };
    let mut dependencies =
        Dependencies::new(config).map_err(|_| "dependency configuration invalid")?;
    dependencies.postgres = pool.clone();
    let auth = AuthAppState::new_with_clock(config, dependencies, Arc::new(SystemClock)).await?;
    let app = accounts_router(
        auth,
        Router::new().route(
            "/health/live",
            get(|| async { Json(json!({"status":"live"})) }),
        ),
    );
    let control = Router::new()
        .route("/__test/material", get(material))
        .route("/__test/hold", post(hold))
        .route("/__test/release", post(release))
        .route("/__test/held", get(held_status))
        .route("/__test/setup-last-admin", post(last_admin))
        .route("/__test/restore-admins", post(last_admin))
        .with_state(state.clone());
    // Bind all sockets before starting tasks so a collision cannot leave a partial harness.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:5341").await?;
    let private_listener = tokio::net::TcpListener::bind("127.0.0.1:5342").await?;
    let worker =
        identity_worker::outbox::MailWorker::new(config, pool.clone(), Arc::new(SystemClock))?;
    let server = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });
    let control = tokio::spawn(async move {
        let _ = axum::serve(private_listener, control).await;
    });
    let worker = tokio::spawn(async move {
        loop {
            let _ = worker.run_once().await;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    });
    println!("ADMIN_MANAGEMENT_READY");
    let stopped = tokio::task::spawn_blocking(|| {
        use std::io::BufRead;
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)
    })
    .await;
    worker.abort();
    control.abort();
    server.abort();
    let _ = worker.await;
    let _ = control.await;
    let _ = server.await;
    let released = match state.held.lock().await.take() {
        Some(lock) => lock.transaction.rollback().await,
        None => Ok(()),
    };
    released.map_err(|_| "private fixture lock cleanup failed")?;
    stopped?.map_err(|_| "harness stop input unavailable")?;
    Ok(())
}

#[tokio::test]
async fn t19_admin_management_harness() -> TestResult {
    if std::env::var("ADMIN_MANAGEMENT_HARNESS").as_deref() != Ok("1") {
        return Err("explicit admin management harness required".into());
    }
    let config = configuration()?;
    let (admin, pool, schema) = isolated(&config).await?;
    let result = run_harness(&config, &pool, &schema).await;
    cleanup(admin, pool, schema).await?;
    result
}
