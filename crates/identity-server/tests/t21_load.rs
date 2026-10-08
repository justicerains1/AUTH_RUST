//! Explicit load fixture and real HTTP service. This harness exists only in the test target.
use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    routing::get,
};
use identity_core::{
    clock::SystemClock,
    config::Config,
    security::{Password, PasswordService, Token, token_digest},
};
use identity_server::accounts::{AuthAppState, accounts_router};
use identity_store::{
    Dependencies,
    migrations::{MigrationTarget, migrate},
};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, QueryBuilder, Row, postgres::PgPoolOptions};
use std::{
    collections::BTreeMap,
    error::Error,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;
use zeroize::Zeroizing;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
struct TokenPair {
    access: Zeroizing<String>,
    refresh: Zeroizing<String>,
    client: usize,
}
struct Client {
    id: Uuid,
    public: String,
    secret: Token,
}
#[derive(Clone)]
struct Control {
    key: Arc<Zeroizing<String>>,
    tokens: Arc<tokio::sync::RwLock<Vec<TokenPair>>>,
    clients: Arc<Vec<Client>>,
    refreshes: Arc<AtomicU64>,
    failures: Arc<AtomicU64>,
    passwords: PasswordService,
    pool: PgPool,
}
fn allowed(state: &Control, request: &Request) -> bool {
    request
        .headers()
        .get("x-test-key")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            identity_core::security::constant_time_equal(value.as_bytes(), state.key.as_bytes())
        })
}
async fn pool(State(state): State<Control>, request: Request) -> (StatusCode, Json<Value>) {
    if !allowed(&state, &request) {
        return (StatusCode::NOT_FOUND, Json(json!({})));
    }
    let tokens = state.tokens.read().await;
    (
        StatusCode::OK,
        Json(
            json!({"tokens":tokens.iter().map(|token|json!({"access":token.access.as_str(),"client":token.client})).collect::<Vec<_>>()}),
        ),
    )
}
async fn stats(State(state): State<Control>, request: Request) -> (StatusCode, Json<Value>) {
    if !allowed(&state, &request) {
        return (StatusCode::NOT_FOUND, Json(json!({})));
    }
    let m = state.passwords.metrics();
    let active = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND state='active'",
    )
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);
    (
        StatusCode::OK,
        Json(
            json!({"refreshes":state.refreshes.load(Ordering::Relaxed),"refresh_failures":state.failures.load(Ordering::Relaxed),"password_hashes":m.hashes,"password_verifications":m.verifications,"password_queue_timeouts":m.queue_timeouts,"password_hash_nanoseconds":m.hash_nanoseconds,"password_verification_nanoseconds":m.verification_nanoseconds,"argon2_memory_kib":m.memory_kib,"argon2_iterations":m.iterations,"argon2_lanes":m.lanes,"pool_size":state.pool.size(),"pool_idle":state.pool.num_idle(),"database_active_connections":active}),
        ),
    )
}
fn configuration() -> TestResult<Config> {
    let mut values: BTreeMap<String, String> = [
        ("APP_ENV", "test"),
        ("BIND", "127.0.0.1:5310"),
        ("ISSUER", "http://localhost:5310"),
        ("RP_ID", "localhost"),
        ("SIGNING_KID", "local-signing-1"),
        ("SMTP_HOST", "127.0.0.1"),
        ("SMTP_PORT", "1025"),
        ("SMTP_FROM", "no-reply@localhost"),
        ("SMTP_TLS", "disabled"),
        ("TRUSTED_PROXY_CIDRS", "127.0.0.1/32"),
        ("DATABASE_POOL_MAX", "32"),
        ("ARGON2_PARALLELISM_LIMIT", "4"),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.into()))
    .collect();
    for field in ["DATABASE_URL", "REDIS_URL"] {
        values.insert(
            field.into(),
            std::env::var(field).map_err(|_| "explicit private load configuration required")?,
        );
    }
    for (key, field) in [
        ("SIGNING_KEY_FILE", "T21_SIGNING_KEY_FILE"),
        ("ENCRYPTION_KEYS_FILE", "T21_ENCRYPTION_KEYS_FILE"),
        ("ACTIVE_ENCRYPTION_KID", "T21_ACTIVE_ENCRYPTION_KID"),
    ] {
        values.insert(
            key.into(),
            std::env::var(field).map_err(|_| "private load keys required")?,
        );
    }
    let environment = std::env::var("APP_ENV").map_err(|_| "APP_ENV=test required")?;
    let database = std::env::var("TEST_DATABASE_URL").map_err(|_| "identity_test required")?;
    MigrationTarget::from_environment(&environment, &database, false, true)?;
    Ok(Config::from_values(&values)?)
}
fn private_path() -> TestResult<PathBuf> {
    let path = PathBuf::from(
        std::env::var("T21_PRIVATE_DIRECTORY").map_err(|_| "private fixture directory required")?,
    );
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".local");
    let resolved = path.canonicalize()?;
    if !resolved.starts_with(root.canonicalize()?) || !resolved.is_dir() {
        return Err("private fixture path rejected".into());
    }
    Ok(resolved)
}
async fn write_private(path: &Path, value: &Value) -> TestResult {
    let bytes = Zeroizing::new(serde_json::to_vec(value)?);
    tokio::fs::write(path, bytes.as_slice()).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
async fn seed(
    pool: &PgPool,
    password: &str,
) -> TestResult<(Vec<Uuid>, Vec<Client>, Vec<(Uuid, Token, Token)>)> {
    let hashing = PasswordService::initialize(4).await?;
    let hash = hashing.hash(&Password::new(password)?).await?;
    let now = OffsetDateTime::now_utc();
    let users = (0..100_000).map(|_| Uuid::new_v4()).collect::<Vec<_>>();
    for batch in users.chunks(1000) {
        let mut query = QueryBuilder::<Postgres>::new(
            "INSERT INTO users(id,email,password_hash,verified,created_at,updated_at) ",
        );
        query.push_values(batch.iter().enumerate(), |mut b, (index, user)| {
            b.push_bind(user)
                .push_bind(format!("t21-{user}-{index}@example.test"))
                .push_bind(hash.as_str())
                .push_bind(true)
                .push_bind(now)
                .push_bind(now);
        });
        query.build().execute(pool).await?;
    }
    let mut clients = Vec::new();
    for index in 0..20 {
        let secret = Token::generate()?;
        let id = Uuid::new_v4();
        let public = format!("t21-client-{index}");
        sqlx::query("INSERT INTO oauth_clients(id,client_id,secret_hash,name,allowed_scopes,created_at,updated_at) VALUES($1,$2,$3,$2,ARRAY['openid','profile','email']::text[],$4,$4)").bind(id).bind(&public).bind(token_digest(secret.expose()).as_slice()).bind(now).execute(pool).await?;
        sqlx::query("INSERT INTO oauth_redirect_uris(id,client_id,uri,kind) VALUES($1,$2,'http://localhost:5310/callback','login')").bind(Uuid::new_v4()).bind(id).execute(pool).await?;
        clients.push(Client { id, public, secret });
    }
    let mut seeds = Vec::new();
    for batch in users.chunks(1000) {
        let rows = batch
            .iter()
            .map(|user| {
                let session = Uuid::new_v4();
                let token = Token::generate()?;
                let csrf = Token::generate()?;
                Ok((*user, session, token, csrf))
            })
            .collect::<TestResult<Vec<_>>>()?;
        let mut q = QueryBuilder::<Postgres>::new(
            "INSERT INTO sessions(id,token_hash,user_id,amr,auth_time,csrf_hash,credential_version,expires_at,created_at) ",
        );
        q.push_values(&rows, |mut b, (user, session, token, csrf)| {
            b.push_bind(session)
                .push_bind(token_digest(token.expose()).to_vec())
                .push_bind(user)
                .push_bind(vec!["pwd"])
                .push_bind(now)
                .push_bind(token_digest(csrf.expose()).to_vec())
                .push_bind(1_i64)
                .push_bind(now + Duration::hours(12))
                .push_bind(now);
        });
        q.build().execute(pool).await?;
        let mut q = QueryBuilder::<Postgres>::new(
            "INSERT INTO oauth_grants(id,user_id,client_id,session_id,scopes,expires_at,created_at) ",
        );
        q.push_values(
            rows.iter().enumerate(),
            |mut b, (index, (user, session, _, _))| {
                b.push_bind(Uuid::new_v4())
                    .push_bind(user)
                    .push_bind(clients[index % 20].id)
                    .push_bind(session)
                    .push_bind(vec!["openid", "profile", "email"])
                    .push_bind(now + Duration::hours(12))
                    .push_bind(now);
            },
        );
        q.build().execute(pool).await?;
        for (_, session, token, csrf) in rows {
            if seeds.len() < 2048 {
                seeds.push((session, token, csrf));
            }
        }
    }
    for table in [
        "users",
        "sessions",
        "oauth_grants",
        "oauth_clients",
        "oauth_tokens",
    ] {
        sqlx::query(sqlx::AssertSqlSafe(format!("ANALYZE {table}")))
            .execute(pool)
            .await?;
    }
    Ok((users, clients, seeds))
}
async fn exchange(
    http: &reqwest::Client,
    pool: &PgPool,
    client: &Client,
    grant: Uuid,
) -> TestResult<TokenPair> {
    let code = Token::generate()?;
    let verifier = Token::generate()?;
    let now = OffsetDateTime::now_utc();
    let challenge = identity_core::oauth::pkce_s256(verifier.expose())?;
    sqlx::query("INSERT INTO authorization_codes(id,code_hash,grant_id,redirect_uri,code_challenge,nonce,expires_at,created_at) VALUES($1,$2,$3,'http://localhost:5310/callback',$4,$5,$6,$7)").bind(Uuid::new_v4()).bind(token_digest(code.expose()).as_slice()).bind(grant).bind(challenge).bind(Token::generate()?.expose()).bind(now+Duration::seconds(60)).bind(now).execute(pool).await?;
    let body = {
        let mut form = url::form_urlencoded::Serializer::new(String::new());
        form.append_pair("grant_type", "authorization_code")
            .append_pair("code", code.expose())
            .append_pair("redirect_uri", "http://localhost:5310/callback")
            .append_pair("code_verifier", verifier.expose());
        form.finish()
    };
    let response = http
        .post("http://127.0.0.1:5310/oauth/token")
        .header(
            "x-forwarded-for",
            format!(
                "198.18.{}.{}",
                grant.as_bytes()[0],
                grant.as_bytes()[1].max(1)
            ),
        )
        .basic_auth(&client.public, Some(client.secret.expose()))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await?;
    if response.status() != reqwest::StatusCode::OK {
        return Err("actual authorization code exchange failed".into());
    }
    let value: Value = response.json().await?;
    Ok(TokenPair {
        access: Zeroizing::new(
            value["access_token"]
                .as_str()
                .ok_or("access missing")?
                .into(),
        ),
        refresh: Zeroizing::new(
            value["refresh_token"]
                .as_str()
                .ok_or("refresh missing")?
                .into(),
        ),
        client: 0,
    })
}
async fn renew(state: Control, http: reqwest::Client) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(120));
    interval.tick().await;
    loop {
        interval.tick().await;
        let count = state.tokens.read().await.len();
        for index in 0..count {
            let (client_index, refresh) = {
                let tokens = state.tokens.read().await;
                (
                    tokens[index].client,
                    Zeroizing::new(tokens[index].refresh.to_string()),
                )
            };
            let client = &state.clients[client_index];
            let body = {
                let mut form = url::form_urlencoded::Serializer::new(String::new());
                form.append_pair("grant_type", "refresh_token")
                    .append_pair("refresh_token", &refresh);
                form.finish()
            };
            match http
                .post("http://127.0.0.1:5310/oauth/token")
                .header("x-forwarded-for", format!("198.18.1.{}", client_index + 1))
                .basic_auth(&client.public, Some(client.secret.expose()))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(body)
                .send()
                .await
            {
                Ok(response) if response.status() == reqwest::StatusCode::OK => {
                    match response.json::<Value>().await {
                        Ok(value) => {
                            if let (Some(access), Some(refresh)) = (
                                value["access_token"].as_str(),
                                value["refresh_token"].as_str(),
                            ) {
                                let mut tokens = state.tokens.write().await;
                                tokens[index].access = Zeroizing::new(access.into());
                                tokens[index].refresh = Zeroizing::new(refresh.into());
                                state.refreshes.fetch_add(1, Ordering::Relaxed);
                            } else {
                                state.failures.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        Err(_) => {
                            state.failures.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                _ => {
                    state.failures.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}
#[tokio::test]
async fn t21_load_harness() -> TestResult {
    if std::env::var("T21_LOAD_HARNESS").as_deref() != Ok("1") {
        return Err("explicit T21 test-only load harness required".into());
    }
    let config = configuration()?;
    let private = private_path()?;
    let target =
        MigrationTarget::from_environment("test", config.database_url.expose(), false, true)?;
    let schema = format!("identity_test_t21_{}", Uuid::new_v4().simple());
    let admin = target.connect().await?;
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await?;
    let pool = PgPoolOptions::new()
        .max_connections(32)
        .connect_with(target.test_schema_options(&schema)?)
        .await?;
    let result = run(&config, &pool, &private).await;
    pool.close().await;
    let cleanup = sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin)
        .await;
    admin.close().await;
    cleanup.map_err(|_| "T21 own schema cleanup failed")?;
    result
}
async fn run(config: &Config, pool: &PgPool, private: &Path) -> TestResult {
    migrate(pool).await?;
    let password = Zeroizing::new(
        std::env::var("T21_PASSWORD").map_err(|_| "private load password required")?,
    );
    let (_, clients, sessions) = seed(pool, &password).await?;
    let mut dependencies = Dependencies::new(config).map_err(|_| "private dependencies invalid")?;
    dependencies.postgres = pool.clone();
    let state = AuthAppState::new_with_clock(config, dependencies, Arc::new(SystemClock)).await?;
    let app = accounts_router(state, Router::new());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:5310").await?;
    let api = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let rows =
        sqlx::query("SELECT id,client_id FROM (SELECT id,client_id,row_number() OVER(PARTITION BY client_id ORDER BY id) AS position FROM oauth_grants) rows WHERE position<=26 ORDER BY position,client_id LIMIT 512")
            .fetch_all(pool)
            .await?;
    let mut tokens = Vec::new();
    for row in rows {
        let client_id: Uuid = row.try_get("client_id")?;
        let index = clients
            .iter()
            .position(|client| client.id == client_id)
            .ok_or("client lookup")?;
        let mut pair = exchange(&http, pool, &clients[index], row.try_get("id")?).await?;
        pair.client = index;
        tokens.push(pair);
    }
    let measured = sqlx::query("SELECT u.id,s.id AS session,g.id AS grant,c.id AS client FROM oauth_grants g JOIN users u ON u.id=g.user_id JOIN sessions s ON s.id=g.session_id JOIN oauth_clients c ON c.id=g.client_id ORDER BY g.id LIMIT 1").fetch_one(pool).await?;
    let account_plan: Value = sqlx::query_scalar("EXPLAIN (ANALYZE,BUFFERS,FORMAT JSON) SELECT u.id,u.email,s.expires_at FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.user_id=$1 AND s.revoked_at IS NULL AND u.verified AND u.status='active'").bind(measured.try_get::<Uuid,_>("id")?).fetch_one(pool).await?;
    let introspection_plan: Value = sqlx::query_scalar("EXPLAIN (ANALYZE,BUFFERS,FORMAT JSON) SELECT t.id,u.id FROM oauth_tokens t JOIN oauth_grants g ON g.id=t.grant_id JOIN sessions s ON s.id=g.session_id JOIN users u ON u.id=g.user_id AND u.id=s.user_id JOIN oauth_clients c ON c.id=g.client_id WHERE g.id=$1 AND c.enabled AND g.revoked_at IS NULL AND s.revoked_at IS NULL AND u.verified AND u.status='active'").bind(measured.try_get::<Uuid,_>("grant")?).fetch_one(pool).await?;
    write_private(&private.join("query-plans.json"), &json!({"account_equivalent_index_path":account_plan,"introspection_equivalent_join_path":introspection_plan,"note":"SQL constants deliberately omit secret token digests; actual endpoints are timed separately."})).await?;
    let credentials = json!({"base":"http://127.0.0.1:5310","control":"http://127.0.0.1:5311","origin":"http://localhost:5310","key":std::env::var("T21_CONTROL_KEY").map_err(|_|"private control key")?,"clients":clients.iter().map(|client|json!({"id":client.public,"secret":client.secret.expose()})).collect::<Vec<_>>(),"sessions":sessions.iter().map(|(_,token,csrf)|json!({"cookie":token.expose(),"csrf":csrf.expose()})).collect::<Vec<_>>(),"password":password.as_str(),"emails":sqlx::query_scalar::<_,String>("SELECT email FROM users ORDER BY created_at,id LIMIT 2048").fetch_all(pool).await?});
    write_private(&private.join("credentials.json"), &credentials).await?;
    let service = PasswordService::initialize(4).await?;
    let control = Control {
        key: Arc::new(Zeroizing::new(
            std::env::var("T21_CONTROL_KEY").map_err(|_| "private control key")?,
        )),
        tokens: Arc::new(tokio::sync::RwLock::new(tokens)),
        clients: Arc::new(clients),
        refreshes: Arc::new(AtomicU64::new(0)),
        failures: Arc::new(AtomicU64::new(0)),
        passwords: service,
        pool: pool.clone(),
    };
    let control_listener = tokio::net::TcpListener::bind("127.0.0.1:5311").await?;
    let control_app = Router::new()
        .route("/pool", get(self::pool))
        .route("/stats", get(stats))
        .with_state(control.clone());
    let control_task = tokio::spawn(async move {
        let _ = axum::serve(control_listener, control_app).await;
    });
    let renew = tokio::spawn(renew(control, http));
    let counts = sqlx::query("SELECT (SELECT count(*) FROM users) AS users,(SELECT count(*) FROM oauth_clients) AS clients,(SELECT count(*) FROM oauth_grants WHERE revoked_at IS NULL AND expires_at>CURRENT_TIMESTAMP) AS grants").fetch_one(pool).await?;
    let seed_report = json!({"users":counts.try_get::<i64,_>("users")?,"clients":counts.try_get::<i64,_>("clients")?,"active_grants":counts.try_get::<i64,_>("grants")?,"token_pool":512,"session_pool":2048,"argon2_memory_kib":65536,"argon2_iterations":3,"argon2_lanes":1,"hash_parallelism":4,"database_pool_max":32,"schema_isolation":true});
    write_private(&private.join("seed-summary.json"), &seed_report).await?;
    println!("T21_LOAD_READY");
    let stop = tokio::task::spawn_blocking(|| {
        let mut input = String::new();
        let _ = std::io::stdin().read_line(&mut input);
    })
    .await;
    renew.abort();
    control_task.abort();
    api.abort();
    stop.map_err(|_| "load harness stop unavailable")?;
    Ok(())
}
