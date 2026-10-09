//! Real administrator/SMTP query observations. No production or credential mutation fixture.
#[path = "../../../tests/load/query-evidence.rs"]
mod query_evidence;
use axum::{Router, extract::Request, middleware::Next, response::Response};
use identity_core::{
    clock::SystemClock,
    config::Config,
    security::{Password, PasswordService, token_digest},
};
use identity_server::accounts::{AuthAppState, accounts_router};
use identity_store::{
    Dependencies,
    admin::AdminStore,
    migrations::{MigrationTarget, migrate},
    repository::Digest,
};
use identity_worker::outbox::MailWorker;
use serde_json::{Value, json};
use sqlx::{ConnectOptions, PgPool, Row, postgres::PgPoolOptions};
use std::{
    collections::BTreeMap,
    error::Error,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};
use time::{Duration, OffsetDateTime};
use tracing::Instrument;
use tracing_subscriber::prelude::*;
use uuid::Uuid;
use zeroize::Zeroizing;
type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

fn configuration() -> TestResult<Config> {
    let mut values = BTreeMap::new();
    for (k, v) in [
        ("APP_ENV", "test"),
        ("BIND", "127.0.0.1:0"),
        ("ISSUER", "http://localhost:5380"),
        ("RP_ID", "localhost"),
        ("SIGNING_KID", "local-signing-1"),
        ("SMTP_HOST", "127.0.0.1"),
        ("SMTP_PORT", "1025"),
        ("SMTP_FROM", "no-reply@localhost"),
        ("SMTP_TLS", "disabled"),
    ] {
        values.insert(k.into(), v.into());
    }
    for field in ["DATABASE_URL", "REDIS_URL"] {
        values.insert(
            field.into(),
            std::env::var(field).map_err(|_| "explicit test dependency required")?,
        );
    }
    for (k, v) in [
        ("SIGNING_KEY_FILE", "T21_OP_SIGNING_KEY_FILE"),
        ("ENCRYPTION_KEYS_FILE", "T21_OP_ENCRYPTION_KEYS_FILE"),
        ("ACTIVE_ENCRYPTION_KID", "T21_OP_ACTIVE_ENCRYPTION_KID"),
    ] {
        values.insert(
            k.into(),
            std::env::var(v).map_err(|_| "private operation key required")?,
        );
    }
    MigrationTarget::from_environment(
        &std::env::var("APP_ENV")?,
        &std::env::var("TEST_DATABASE_URL")?,
        false,
        true,
    )?;
    Ok(Config::from_values(&values)?)
}
async fn observe(request: Request, next: Next) -> Response {
    let endpoint = match (request.method().as_str(), request.uri().path()) {
        ("GET", "/api/v1/admin/users") => "admin_users",
        ("GET", "/api/v1/admin/clients") => "admin_clients",
        ("GET", "/api/v1/admin/audit-events") => "admin_audit",
        _ => "setup",
    };
    let span = tracing::info_span!("t21_endpoint", endpoint, status = tracing::field::Empty);
    let response = next.run(request).instrument(span.clone()).await;
    span.record("status", u64::from(response.status().as_u16()));
    response
}
struct Browser {
    client: reqwest::Client,
    base: String,
    cookie: Zeroizing<String>,
    csrf: Zeroizing<String>,
}
impl Browser {
    async fn new(base: &str) -> TestResult<Self> {
        let client = reqwest::Client::new();
        let response = client
            .get(format!("{base}/api/v1/auth/csrf"))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err("operation csrf failed".into());
        }
        let cookie = response
            .headers()
            .get("set-cookie")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .ok_or("csrf cookie missing")?
            .to_owned();
        let body: Value = response.json().await?;
        Ok(Self {
            client,
            base: base.into(),
            cookie: Zeroizing::new(cookie),
            csrf: Zeroizing::new(body["csrf_token"].as_str().ok_or("csrf missing")?.into()),
        })
    }
    async fn post(&self, path: &str, body: Value) -> TestResult<reqwest::Response> {
        Ok(self
            .client
            .post(format!("{}/api/v1{path}", self.base))
            .header("origin", "http://localhost:5380")
            .header("cookie", self.cookie.as_str())
            .header("x-csrf-token", self.csrf.as_str())
            .json(&body)
            .send()
            .await?)
    }
    async fn login(&mut self, email: &str, password: &str) -> TestResult {
        let response = self
            .post(
                "/auth/login/password",
                json!({"email":email,"password":password}),
            )
            .await?;
        if !response.status().is_success() {
            return Err("operation password login failed".into());
        }
        let cookies = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .filter(|v| !v.contains("Max-Age=0"))
            .filter_map(|v| v.split(';').next())
            .collect::<Vec<_>>()
            .join("; ");
        let value: Value = response.json().await?;
        if value["status"] != "authenticated" {
            return Err("operation login not authenticated".into());
        }
        self.cookie = Zeroizing::new(cookies);
        self.csrf = Zeroizing::new(
            value["csrf_token"]
                .as_str()
                .ok_or("login csrf missing")?
                .into(),
        );
        Ok(())
    }
    async fn get(&self, path: &str) -> TestResult<Value> {
        let response = self
            .client
            .get(format!("{}/api/v1{path}", self.base))
            .header("cookie", self.cookie.as_str())
            .send()
            .await?;
        if response.status() != reqwest::StatusCode::OK {
            eprintln!(
                "Operational fixed endpoint HTTP status={}",
                response.status().as_u16()
            );
            return Err("operation request rejected".into());
        }
        Ok(response.json().await?)
    }
}
async fn strengthen(browser: &Browser, password: &str) -> TestResult {
    if browser
        .post("/me/reauth/password", json!({"password":password}))
        .await?
        .status()
        != reqwest::StatusCode::OK
    {
        return Err("recent password authentication failed".into());
    }
    let options = browser.post("/me/mfa/totp/enrollment", json!({})).await?;
    if options.status() != reqwest::StatusCode::OK {
        return Err("factor enrollment rejected".into());
    }
    let options: Value = options.json().await?;
    let secret = options["secret"].as_str().ok_or("factor secret missing")?;
    let code = rfc_totp(secret, OffsetDateTime::now_utc().unix_timestamp())?;
    let confirmed = browser
        .post(
            "/me/mfa/totp/enrollment/confirm",
            json!({"challenge_id":options["challenge_id"],"code":code.as_str()}),
        )
        .await?;
    if confirmed.status() != reqwest::StatusCode::OK {
        return Err("real factor proof rejected".into());
    }
    Ok(())
}
fn rfc_totp(secret: &str, seconds: i64) -> TestResult<Zeroizing<String>> {
    let mut bits = 0;
    let mut buffer = 0_u32;
    let mut bytes = Vec::new();
    for ch in secret.chars() {
        let value = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"
            .find(ch)
            .ok_or("base32 factor malformed")? as u32;
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            bytes.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    let mut child=std::process::Command::new(std::env::var("T21_OP_NODE")?).args(["-e","const fs=require('node:fs'),crypto=require('node:crypto');const[s,c]=fs.readFileSync(0,'utf8').trim().split(/\\s+/);const d=crypto.createHmac('sha1',Buffer.from(s,'hex')).update(Buffer.from(c,'hex')).digest();process.stdout.write(String((d.readUInt32BE(d[19]&15)&0x7fffffff)%1000000).padStart(6,'0'));"]).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn()?;
    use std::io::Write;
    if let Some(mut input) = child.stdin.take() {
        let value = Zeroizing::new(format!(
            "{} {}",
            bytes.iter().map(|v| format!("{v:02x}")).collect::<String>(),
            u64::try_from(seconds / 30)?
                .to_be_bytes()
                .iter()
                .map(|v| format!("{v:02x}"))
                .collect::<String>()
        ));
        input.write_all(value.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err("independent RFC proof failed".into());
    }
    Ok(Zeroizing::new(String::from_utf8(output.stdout)?))
}
async fn write_report(private: &Path, name: &str, value: &Value) -> TestResult {
    let path = private.join(name);
    tokio::fs::write(&path, serde_json::to_vec_pretty(value)?).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
async fn finished(observer: &query_evidence::Observer, endpoint: &str) -> TestResult<Value> {
    for _ in 0..100 {
        let value = observer.snapshot();
        if value["endpoints"][endpoint]["requests"] == 1 {
            return Ok(value);
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    Err("operation SQL completion unavailable".into())
}
fn validate_count(value: &Value, endpoint: &str) -> TestResult<u64> {
    let entry = &value["endpoints"][endpoint];
    if entry["unknown_statement_count"] != 0
        || entry["sql_per_request_min"] != entry["sql_per_request_max"]
    {
        return Err("unknown SQL or missing elapsed in operation".into());
    }
    let count = entry["sql_per_request_min"]
        .as_u64()
        .ok_or("operation statement count missing")?;
    if count == 0
        || entry["query_completions"]
            .as_array()
            .ok_or("operation queries missing")?
            .iter()
            .map(|q| q["timing"]["count"].as_u64().unwrap_or(0))
            .sum::<u64>()
            != count
    {
        return Err("operation completion count differs from SQL timing".into());
    }
    Ok(count)
}

#[tokio::test]
async fn t21_operational_queries() -> TestResult {
    if std::env::var("T21_OPERATIONAL_PROBE").as_deref() != Ok("1") {
        return Err("explicit operational SQL probe required".into());
    }
    let config = configuration()?;
    let private = PathBuf::from(std::env::var("T21_OP_PRIVATE_DIRECTORY")?);
    let local = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local")
        .canonicalize()?;
    if !private.canonicalize()?.starts_with(local) {
        return Err("private output directory rejected".into());
    }
    let observer = query_evidence::Observer::new();
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(observer.clone()))
        .map_err(|_| "observer install failed")?;
    let target =
        MigrationTarget::from_environment("test", config.database_url.expose(), false, true)?;
    let schema = format!("identity_test_t21_operational_{}", Uuid::new_v4().simple());
    let admin = target.connect().await?;
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await?;
    let options = target
        .test_schema_options(&schema)?
        .application_name("t21_operational_probe")
        .log_statements("debug".parse()?)
        .log_slow_statements("debug".parse()?, std::time::Duration::from_millis(100));
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .acquire_timeout(std::time::Duration::from_secs(2))
        .acquire_time_level("debug".parse()?)
        .acquire_slow_level("debug".parse()?)
        .connect_with(options)
        .await?;
    let result = run(&config, &pool, &private, &observer).await;
    pool.close().await;
    let cleanup = sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin)
        .await;
    admin.close().await;
    cleanup.map_err(|_| "operational schema cleanup failed")?;
    if let Err(error) = &result {
        if let Some(db) = error.downcast_ref::<sqlx::Error>() {
            eprintln!(
                "Operational SQL failed; database code={}",
                db.as_database_error()
                    .and_then(|v| v.code())
                    .unwrap_or_default()
            );
        } else {
            let text = error.to_string();
            if text.len() < 150 && !text.contains('@') {
                eprintln!("Operational SQL rejected: {text}");
            }
        }
    }
    result.map_err(|_| "operational query probe failed; sanitized evidence retained".into())
}
async fn run(
    config: &Config,
    pool: &PgPool,
    private: &Path,
    observer: &query_evidence::Observer,
) -> TestResult {
    migrate(pool).await?;
    let password = Zeroizing::new(std::env::var("T21_OP_PASSWORD")?);
    let passwords = PasswordService::initialize(4).await?;
    let hash = passwords.hash(&Password::new(&password)?).await?;
    // Bulk fixture rows are not an authentication proof. The admin verifies the same real hash.
    for batch in 0..100 {
        let mut q = sqlx::QueryBuilder::<sqlx::Postgres>::new(
            "INSERT INTO users(id,email,password_hash,verified,created_at,updated_at) ",
        );
        q.push_values(0..1000, |mut b, index| {
            let id = Uuid::new_v4();
            b.push_bind(id)
                .push_bind(format!("operational-{batch}-{index}-{id}@example.test"))
                .push_bind(hash.as_str())
                .push_bind(true)
                .push_bind(OffsetDateTime::now_utc())
                .push_bind(OffsetDateTime::now_utc());
        });
        q.build().execute(pool).await?;
    }
    let selected = sqlx::query("SELECT id,email FROM users ORDER BY id LIMIT 1")
        .fetch_one(pool)
        .await?;
    let actor: Uuid = selected.try_get("id")?;
    let email: Zeroizing<String> = Zeroizing::new(selected.try_get("email")?);
    let store = AdminStore::new(pool.clone(), Arc::new(SystemClock));
    let proof = store
        .verify_bootstrap(
            email.to_string(),
            &password,
            &passwords,
            Uuid::new_v4(),
            Digest::from_bytes(token_digest("operation-synthetic-source")),
        )
        .await?;
    let boot = store.bootstrap_verified(&proof).await?;
    if boot.user_id != actor {
        return Err("verified bootstrap changed actor".into());
    }
    let mut dependencies =
        Dependencies::new(config).map_err(|_| "operational dependencies invalid")?;
    dependencies.postgres = pool.clone();
    let state = AuthAppState::new(config, dependencies).await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let app = accounts_router(state, Router::new()).layer(axum::middleware::from_fn(observe));
    let server = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });
    let result = operations(config, pool, private, observer, &base, &email, &password).await;
    server.abort();
    result
}
async fn operations(
    config: &Config,
    pool: &PgPool,
    private: &Path,
    observer: &query_evidence::Observer,
    base: &str,
    email: &str,
    password: &str,
) -> TestResult {
    let mut browser = Browser::new(base).await?;
    browser.login(email, password).await?;
    strengthen(&browser, password).await?;
    let mut expected_clients = BTreeMap::new();
    for index in 0..21 {
        let response=browser.post("/admin/clients",json!({"name":format!("Operational fixture {index}"),"allowed_scopes":["openid","email"],"redirect_uris":[format!("http://localhost:5380/callback/{index}/z"),format!("http://localhost:5380/callback/{index}/a")],"post_logout_redirect_uris":[format!("http://localhost:5380/logout/{index}/z"),format!("http://localhost:5380/logout/{index}/a")]})).await?;
        if response.status() != reqwest::StatusCode::CREATED {
            return Err("real administrator client create rejected".into());
        }
        let body: Value = response.json().await?;
        let client = body["client"].clone();
        let id = client["id"]
            .as_str()
            .ok_or("created client id missing")?
            .to_string();
        expected_clients.insert(id, client);
    }
    sqlx::query("ANALYZE users,oauth_clients,oauth_redirect_uris,audit_events")
        .execute(pool)
        .await?;
    let mut observations = Vec::new();
    let mut unequal = Vec::new();
    let mut client_page_checks = Vec::new();
    for (endpoint, path) in [
        ("admin_users", "/admin/users"),
        ("admin_clients", "/admin/clients"),
        ("admin_audit", "/admin/audit-events"),
    ] {
        let mut counts = Vec::new();
        let mut first_id = None;
        for limit in [1, 20] {
            observer.reset();
            let body = browser.get(&format!("{path}?limit={limit}")).await?;
            if body["items"]
                .as_array()
                .is_none_or(|rows| rows.len() != limit)
            {
                return Err("administrator page item count incorrect".into());
            }
            let items = body["items"].as_array().ok_or("page missing")?;
            if limit == 1 {
                first_id = items[0]["id"].as_str().map(str::to_owned);
            } else if first_id.as_deref() != items[0]["id"].as_str() {
                return Err("page order changed across limits".into());
            }
            if endpoint == "admin_clients" {
                for client in items {
                    let id = client["id"].as_str().ok_or("listed client id missing")?;
                    if expected_clients.get(id) != Some(client) {
                        return Err(
                            "listed client scope redirect data differs from created client".into(),
                        );
                    }
                    if client["redirect_uris"]
                        .as_array()
                        .is_none_or(|items| items.len() != 2)
                        || client["post_logout_redirect_uris"]
                            .as_array()
                            .is_none_or(|items| items.len() != 2)
                    {
                        return Err("client callback lists incomplete".into());
                    }
                }
            }
            let observed = finished(observer, endpoint).await?;
            let count = validate_count(&observed, endpoint)?;
            counts.push(count);
            observations.push(json!({"endpoint":endpoint,"page_size":limit,"sql_count":count,"observation":observed}));
            if endpoint == "admin_clients" && limit == 20 {
                let cursor = body["next_cursor"]
                    .as_str()
                    .ok_or("client pagination cursor missing")?;
                let seen = items
                    .iter()
                    .filter_map(|row| row["id"].as_str())
                    .collect::<Vec<_>>();
                let next = browser
                    .get(&format!("{path}?limit=20&cursor={cursor}"))
                    .await?;
                let next_items = next["items"].as_array().ok_or("next client page missing")?;
                if next_items.len() != 1 || next["next_cursor"] != Value::Null {
                    return Err("final client page incorrect".into());
                }
                for client in next_items {
                    let id = client["id"].as_str().ok_or("next client id missing")?;
                    if seen.contains(&id) || expected_clients.get(id) != Some(client) {
                        return Err("client pagination duplicate or altered content".into());
                    }
                }
                let empty = browser
                    .get("/admin/clients?limit=1&cursor=invalid-cursor")
                    .await;
                if empty.is_ok() {
                    return Err("invalid cursor was accepted".into());
                }
                client_page_checks.push(json!({"next_page_rows":1,"same_client_content":true,"no_duplicate_ids":true,"cursor_exhausted":true,"invalid_cursor_rejected":true}));
            }
        }
        if counts[0] != counts[1] {
            unequal.push(
                json!({"endpoint":endpoint,"page_one_sql":counts[0],"page_twenty_sql":counts[1]}),
            );
        }
    }
    write_report(private,"operational-query-evidence.json",&json!({"stage":"admin-pagination-complete","seed_users":100000,"administrator_pages":observations,"detected_query_growth":unequal})).await?;
    let plans = operational_plans(pool, &browser).await?;
    // Empty-page check uses a repository API with the same genuinely authenticated admin session.
    // Remove only this test-created client set after exact plans and page/content assertions.
    let cookie = browser
        .cookie
        .split(';')
        .map(str::trim)
        .find_map(|pair| pair.strip_prefix("identity-dev="))
        .ok_or("admin identity cookie missing")?;
    let context = identity_store::admin::AdminContext {
        session_hash: Digest::from_bytes(token_digest(cookie)),
        request_id: Uuid::new_v4(),
        source_hash: Digest::from_bytes(token_digest("operational-fixture-source")),
    };
    let key = identity_core::security::AeadKeyRing::load_file(
        &config.encryption_keys_file,
        &config.active_encryption_kid,
    )?
    .derive_hmac_key("operational-cursor")?;
    let ids = expected_clients
        .keys()
        .map(|id| Uuid::parse_str(id))
        .collect::<Result<Vec<_>, _>>()?;
    // Explicit ordering fixture for equal timestamps; no authentication fact is changed.
    sqlx::query("UPDATE oauth_clients SET created_at=$2,updated_at=$2 WHERE id=ANY($1)")
        .bind(&ids)
        .bind(OffsetDateTime::now_utc())
        .execute(pool)
        .await?;
    let expected_order = ids.iter().rev().map(Uuid::to_string).collect::<Vec<_>>();
    // Repeated repository paging uses actual signed cursors without consuming the HTTP Admin budget.
    let store = AdminStore::new(pool.clone(), Arc::new(SystemClock));
    let mut cursor = None;
    let mut seen = 0_usize;
    let mut actual_order = Vec::new();
    loop {
        let page = store.clients(context, 1, cursor.as_deref(), &key).await?;
        actual_order.extend(page.items.iter().map(|client| client.id.to_string()));
        seen += page.items.len();
        if let Some(next) = page.next_cursor {
            cursor = Some(next);
        } else {
            break;
        }
        if seen > 21 {
            return Err("client cursor loop did not terminate".into());
        }
    }
    if seen != 21 {
        return Err("valid cursor did not return all fixture clients".into());
    }
    if actual_order != expected_order {
        return Err("client tied timestamp cursor order incorrect".into());
    }
    let selected = ids[0];
    sqlx::query("DELETE FROM oauth_redirect_uris WHERE client_id=$1 AND kind='logout'")
        .bind(selected)
        .execute(pool)
        .await?;
    sqlx::query("UPDATE oauth_clients SET enabled=false WHERE id=$1")
        .bind(selected)
        .execute(pool)
        .await?;
    let changed = store.clients(context, 100, None, &key).await?;
    let selected_view = changed
        .items
        .iter()
        .find(|client| client.id == selected)
        .ok_or("disabled client omitted from admin list")?;
    if selected_view.enabled
        || !selected_view.post_logout_redirect_uris.is_empty()
        || selected_view.redirect_uris.len() != 2
    {
        return Err("disabled or no logout callback list changed".into());
    }
    let ids = expected_clients
        .keys()
        .map(|id| Uuid::parse_str(id))
        .collect::<Result<Vec<_>, _>>()?;
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM oauth_redirect_uris WHERE client_id=ANY($1)")
        .bind(&ids)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM oauth_clients WHERE id=ANY($1)")
        .bind(&ids)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let empty = store.clients(context, 20, None, &key).await?;
    if !empty.items.is_empty() || empty.next_cursor.is_some() {
        return Err("authorized empty client list incorrect".into());
    }
    client_page_checks.push(json!({"repository_valid_cursor_rows":seen,"timestamp_tie_id_order":true,"disabled_client_listed":true,"no_logout_callbacks_remain_empty":true,"authorized_empty_rows":0,"empty_next_cursor":false}));
    let worker = MailWorker::new(config, pool.clone(), Arc::new(SystemClock))?;
    let drained = worker.run_once().await?;
    if drained.retried != 0 || drained.failed != 0 {
        return Err("setup SMTP drain failed".into());
    }
    let mut worker_records = Vec::new();
    for number in [1, 10] {
        let mut recipients = Vec::new();
        for index in 0..number {
            let email = format!(
                "operation-mail-{}-{index}@example.test",
                Uuid::new_v4().simple()
            );
            let response = browser
                .post("/auth/register", json!({"email":email,"password":password}))
                .await?;
            if response.status() != reqwest::StatusCode::ACCEPTED {
                return Err("real operational registration rejected".into());
            }
            recipients.push(email);
        }
        observer.reset();
        let span = tracing::info_span!(
            "t21_endpoint",
            endpoint = "worker_batch",
            status = tracing::field::Empty
        );
        let batch = worker.run_once().instrument(span.clone()).await?;
        span.record("status", 200_u64);
        drop(span);
        if batch.claimed != number as u64
            || batch.delivered != number as u64
            || batch.retried != 0
            || batch.failed != 0
        {
            return Err("real worker batch delivery incomplete".into());
        }
        let observed = finished(observer, "worker_batch").await?;
        let count = validate_count(&observed, "worker_batch")?;
        if count != 2 + 3 * number as u64 {
            return Err(
                "worker query count differs from one claim and per-message commit audit update"
                    .into(),
            );
        }
        let claim = query_evidence::source_sql("repository", "claim_outbox", "WITH picked")?;
        let complete = query_evidence::source_sql(
            "worker_outbox",
            "delivery_committed",
            "UPDATE email_outbox",
        )?;
        let audit = query_evidence::source_sql(
            "worker_outbox",
            "delivery_audit",
            "INSERT INTO audit_events",
        )?;
        let queries = observed["endpoints"]["worker_batch"]["query_completions"]
            .as_array()
            .ok_or("worker query shapes missing")?;
        for (sql, expected) in [
            (claim, 1_u64),
            (complete, number as u64),
            (audit, number as u64),
            ("COMMIT".into(), number as u64 + 1),
        ] {
            let id = query_evidence::fingerprint(&sql);
            let actual = queries
                .iter()
                .find(|q| q["shape_id"] == id)
                .and_then(|q| q["timing"]["count"].as_u64());
            if actual != Some(expected) {
                return Err("worker completion statement count incomplete".into());
            }
        }
        for email in recipients {
            assert_mail_delivered(&browser.client, &email).await?;
            let cleared:i64=sqlx::query_scalar("SELECT count(*) FROM email_outbox WHERE recipient=$1 AND state='delivered' AND encrypted_params IS NULL").bind(email).fetch_one(pool).await?;
            if cleared != 1 {
                return Err("worker payload was not cleared".into());
            }
        }
        worker_records.push(json!({"delivered":number,"sql_count":count,"observation":observed}));
    }
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(pool)
        .await?;
    let report = json!({"scope":"local real admin strong-auth pagination and SMTP worker SQL observations; no capacity or production claim","seed_users":100000,"final_user_count":users,"administrator_authentication":"same verified bootstrap password, real HTTP login/recent password/TOTP proof; no strong_at mutation","administrator_pages":observations,"client_page_checks":client_page_checks,"detected_query_growth":unequal,"worker_batches":worker_records,"exact_plans":plans,"secret_policy":"only parameterized source SQL and positive plan fields; no bound user/credential/mail values"});
    write_report(private, "operational-query-evidence.json", &report).await?;
    if !unequal.is_empty() {
        return Err("administrator pagination contains per-row SQL growth".into());
    }
    Ok(())
}
async fn assert_mail_delivered(client: &reqwest::Client, email: &str) -> TestResult {
    for _ in 0..30 {
        let value: Value = client
            .get("http://127.0.0.1:8025/api/v1/messages")
            .send()
            .await?
            .json()
            .await?;
        if value["messages"].as_array().is_some_and(|items| {
            items.iter().any(|mail| {
                mail["To"]
                    .as_array()
                    .is_some_and(|to| to.iter().any(|target| target["Address"] == email))
            })
        }) {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Err("actual SMTP message not present".into())
}
async fn operational_plans(pool: &PgPool, browser: &Browser) -> TestResult<Value> {
    let mut plans = Vec::new();
    let mut tx = pool.begin().await?;
    let cookie = browser
        .cookie
        .split(';')
        .map(str::trim)
        .find_map(|pair| pair.strip_prefix("identity-dev="))
        .ok_or("identity cookie missing")?;
    let hash = token_digest(cookie);
    let actor: Uuid = sqlx::query_scalar("SELECT user_id FROM sessions WHERE token_hash=$1")
        .bind(hash.as_slice())
        .fetch_one(&mut *tx)
        .await?;
    let now = OffsetDateTime::now_utc();
    macro_rules! plan{($module:literal,$function:literal,$prefix:literal;$($binding:expr),*)=>{{let sql=query_evidence::source_sql($module,$function,$prefix)?;let raw=sqlx::query_scalar::<_,Value>(sqlx::AssertSqlSafe(format!("EXPLAIN (ANALYZE,BUFFERS,FORMAT JSON) {sql}")))$(.bind($binding))*.fetch_one(&mut *tx).await?;plans.push(json!({"module":$module,"function":$function,"parameterized_sql":sql,"shape_id":query_evidence::fingerprint(&sql),"plan":query_evidence::safe_plan(&raw)?}));}};}
    plan!("admin","authorize","SELECT s.id";hash.to_vec(),now);
    plan!("admin","has_factor","SELECT EXISTS";actor);
    plan!("admin","filtered_users","SELECT id,email";None::<OffsetDateTime>,None::<Uuid>,21_i64,None::<String>,None::<String>);
    plan!("admin","clients","SELECT id,client_id";None::<OffsetDateTime>,None::<Uuid>,21_i64);
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM oauth_clients ORDER BY created_at DESC,id DESC LIMIT 20",
    )
    .fetch_all(&mut *tx)
    .await?;
    plan!("admin","clients","SELECT client_id,kind,uri";ids);
    plan!("admin","filtered_audit_events","SELECT id,event";None::<OffsetDateTime>,None::<Uuid>,21_i64,None::<OffsetDateTime>,None::<OffsetDateTime>);
    plan!("repository","claim_outbox","WITH picked";now,10_i64,Uuid::new_v4(),now+Duration::minutes(5));
    let completion = sqlx::query("SELECT id,lease_id FROM email_outbox LIMIT 1")
        .fetch_optional(&mut *tx)
        .await?;
    if let Some(row) = completion {
        plan!("worker_outbox","delivery_committed","UPDATE email_outbox";row.try_get::<Uuid,_>("id")?,row.try_get::<Option<Uuid>,_>("lease_id")?,now);
    }
    tx.rollback().await?;
    Ok(json!(plans))
}
