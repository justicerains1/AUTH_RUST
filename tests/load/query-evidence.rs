//! Test-only SQL completion observer. Bind values and raw plans never leave this module.
use identity_core::security::token_digest;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Instant,
};
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id},
};
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};

const SOURCES: &[(&str, &str)] = &[
    (
        "accounts",
        include_str!("../../crates/identity-store/src/accounts.rs"),
    ),
    (
        "admin",
        include_str!("../../crates/identity-store/src/admin.rs"),
    ),
    (
        "mfa",
        include_str!("../../crates/identity-store/src/mfa.rs"),
    ),
    (
        "oauth",
        include_str!("../../crates/identity-store/src/oauth.rs"),
    ),
    (
        "passkeys",
        include_str!("../../crates/identity-store/src/passkeys.rs"),
    ),
    (
        "passwords",
        include_str!("../../crates/identity-store/src/passwords.rs"),
    ),
    (
        "repository",
        include_str!("../../crates/identity-store/src/repository.rs"),
    ),
    (
        "security",
        include_str!("../../crates/identity-store/src/security.rs"),
    ),
    (
        "sessions",
        include_str!("../../crates/identity-store/src/sessions.rs"),
    ),
    (
        "tokens",
        include_str!("../../crates/identity-store/src/tokens.rs"),
    ),
];
pub fn normalize(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(';')
        .to_string()
}
pub fn fingerprint(value: &str) -> String {
    token_digest(&normalize(value))
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn literals(source: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut chars = source.chars();
    while let Some(ch) = chars.next() {
        if ch != '"' {
            continue;
        }
        let mut value = String::new();
        let mut escaped = false;
        for ch in chars.by_ref() {
            if escaped {
                value.push(match ch {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    other => other,
                });
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                break;
            } else {
                value.push(ch);
            }
        }
        let normalized = normalize(&value);
        if ["SELECT ", "INSERT ", "UPDATE ", "DELETE ", "WITH "]
            .iter()
            .any(|prefix| normalized.starts_with(prefix))
        {
            values.push(normalized);
        }
    }
    values
}
pub fn source_sql(module: &str, function: &str, starts: &str) -> Result<String, &'static str> {
    let source = SOURCES
        .iter()
        .find(|(name, _)| *name == module)
        .ok_or("unknown repository source")?
        .1;
    let position = source
        .find(&format!("fn {function}("))
        .ok_or("repository function missing")?;
    let rest = &source[position..];
    let end = rest[3..]
        .find("\n    pub ")
        .map_or(rest.len(), |index| index + 3);
    let matches = literals(&rest[..end])
        .into_iter()
        .filter(|sql| sql.starts_with(starts))
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err("repository SQL shape missing or ambiguous");
    }
    Ok(matches[0].clone())
}

#[derive(Default, Clone)]
struct Timing {
    count: u64,
    total_ns: u64,
    max_ns: u64,
    buckets: [u64; 8],
}
impl Timing {
    fn record(&mut self, seconds: f64) {
        if !seconds.is_finite() || seconds < 0.0 {
            return;
        }
        let ns = (seconds * 1e9).min(u64::MAX as f64) as u64;
        self.count += 1;
        self.total_ns = self.total_ns.saturating_add(ns);
        self.max_ns = self.max_ns.max(ns);
        let bounds = [
            100_000,
            500_000,
            1_000_000,
            5_000_000,
            25_000_000,
            100_000_000,
            250_000_000,
            u64::MAX,
        ];
        for (count, bound) in self.buckets.iter_mut().zip(bounds) {
            if ns <= bound {
                *count += 1;
            }
        }
    }
    fn json(&self) -> Value {
        json!({"count":self.count,"total_nanoseconds":self.total_ns,"max_nanoseconds":self.max_ns,"bucket_le_nanoseconds":[100_000,500_000,1_000_000,5_000_000,25_000_000,100_000_000,250_000_000],"cumulative_buckets":self.buckets})
    }
}
#[derive(Default, Clone)]
struct QueryCount {
    timing: Timing,
    returned: u64,
    affected: u64,
}
#[derive(Default, Clone)]
struct RequestCount {
    requests: u64,
    sql_min: u64,
    sql_max: u64,
    queries: BTreeMap<String, QueryCount>,
    status: BTreeMap<u16, u64>,
    duration: Timing,
    acquisition: Timing,
    unknown: u64,
}
#[derive(Clone)]
pub struct Observer {
    totals: Arc<Mutex<BTreeMap<String, RequestCount>>>,
    catalog: Arc<BTreeMap<String, (String, String)>>,
}
struct Observation {
    endpoint: String,
    started: Instant,
    count: u64,
    status: u16,
    queries: BTreeMap<String, QueryCount>,
    acquisition: Timing,
    unknown: u64,
}
#[derive(Default)]
struct Fields {
    endpoint: Option<String>,
    statement: Option<String>,
    summary: Option<String>,
    elapsed: Option<f64>,
    acquired: Option<f64>,
    returned: u64,
    affected: u64,
    status: Option<u16>,
}
impl Visit for Fields {
    fn record_str(&mut self, f: &Field, v: &str) {
        match f.name() {
            "endpoint" => self.endpoint = Some(v.into()),
            "db.statement" => self.statement = Some(v.into()),
            "summary" => self.summary = Some(v.into()),
            _ => {}
        }
    }
    fn record_f64(&mut self, f: &Field, v: f64) {
        match f.name() {
            "elapsed_secs" => self.elapsed = Some(v),
            "acquired_after_secs" => self.acquired = Some(v),
            _ => {}
        }
    }
    fn record_u64(&mut self, f: &Field, v: u64) {
        match f.name() {
            "rows_returned" => self.returned = v,
            "rows_affected" => self.affected = v,
            "status" => self.status = u16::try_from(v).ok(),
            _ => {}
        }
    }
    fn record_debug(&mut self, f: &Field, v: &dyn std::fmt::Debug) {
        if matches!(f.name(), "db.statement" | "summary") {
            let formatted = format!("{v:?}");
            if let Ok(value) = serde_json::from_str::<String>(&formatted) {
                self.record_str(f, &value);
            }
        }
    }
}
impl Observer {
    pub fn new() -> Self {
        let mut catalog = BTreeMap::new();
        for (module, source) in SOURCES {
            for sql in literals(source) {
                catalog.insert(
                    fingerprint(&sql),
                    (format!("crates/identity-store/src/{module}.rs"), sql),
                );
            }
        }
        for sql in ["BEGIN", "COMMIT", "ROLLBACK", "SELECT 1"] {
            catalog.insert(
                fingerprint(sql),
                ("SQLx transaction lifecycle".into(), sql.into()),
            );
        }
        Self {
            totals: Arc::new(Mutex::new(BTreeMap::new())),
            catalog: Arc::new(catalog),
        }
    }
    pub fn snapshot(&self) -> Value {
        let totals = self
            .totals
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let endpoints=totals.iter().map(|(endpoint,count)| {let queries=count.queries.iter().map(|(id,q)|{let source=self.catalog.get(id);json!({"shape_id":id,"source":source.map(|item|item.0.as_str()),"parameterized_sql":source.map(|item|item.1.as_str()),"timing":q.timing.json(),"rows_returned":q.returned,"rows_affected":q.affected})}).collect::<Vec<_>>();(endpoint.clone(),json!({"requests":count.requests,"sql_per_request_min":count.sql_min,"sql_per_request_max":count.sql_max,"query_completions":queries,"statuses":count.status,"request_duration":count.duration.json(),"pool_acquisition":count.acquisition.json(),"unknown_statement_count":count.unknown}))}).collect::<BTreeMap<_,_>>();
        json!({"scope":"actual SQLx completion events inside fixed endpoint spans; no bind values","endpoints":endpoints})
    }
    pub fn reset(&self) {
        self.totals
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
}
impl<S> Layer<S> for Observer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn enabled(&self, metadata: &tracing::Metadata<'_>, _ctx: Context<'_, S>) -> bool {
        metadata.target().starts_with("sqlx::") || metadata.name() == "t21_endpoint"
    }
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        if attrs.metadata().name() != "t21_endpoint" {
            return;
        }
        let mut f = Fields::default();
        attrs.record(&mut f);
        if let (Some(endpoint), Some(span)) = (f.endpoint, ctx.span(id)) {
            span.extensions_mut().insert(Observation {
                endpoint,
                started: Instant::now(),
                count: 0,
                status: 0,
                queries: BTreeMap::new(),
                acquisition: Timing::default(),
                unknown: 0,
            });
        }
    }
    fn on_record(&self, id: &Id, values: &tracing::span::Record<'_>, ctx: Context<'_, S>) {
        let mut f = Fields::default();
        values.record(&mut f);
        if let (Some(status), Some(span)) = (f.status, ctx.span(id))
            && let Some(observation) = span.extensions_mut().get_mut::<Observation>()
        {
            observation.status = status;
        }
    }
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let Some(scope) = ctx.event_scope(event) else {
            return;
        };
        let mut f = Fields::default();
        event.record(&mut f);
        for span in scope.from_root() {
            let mut extensions = span.extensions_mut();
            let Some(observation) = extensions.get_mut::<Observation>() else {
                continue;
            };
            if event.metadata().target() == "sqlx::pool::acquire" {
                if let Some(seconds) = f.acquired {
                    observation.acquisition.record(seconds);
                }
                return;
            }
            if event.metadata().target() != "sqlx::query" {
                return;
            }
            let sql = f
                .statement
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .or(f.summary.as_deref())
                .map(normalize)
                .unwrap_or_default();
            let id = fingerprint(&sql);
            observation.count += 1;
            if !self.catalog.contains_key(&id) {
                observation.unknown += 1;
            }
            let entry = observation.queries.entry(id).or_default();
            if let Some(elapsed) = f.elapsed {
                entry.timing.record(elapsed);
            } else {
                observation.unknown += 1;
            }
            entry.returned += f.returned;
            entry.affected += f.affected;
            return;
        }
    }
    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else {
            return;
        };
        let mut extensions = span.extensions_mut();
        let Some(observation) = extensions.remove::<Observation>() else {
            return;
        };
        let mut totals = self
            .totals
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let total = totals.entry(observation.endpoint).or_default();
        if total.requests == 0 {
            total.sql_min = observation.count;
        } else {
            total.sql_min = total.sql_min.min(observation.count);
        }
        total.requests += 1;
        total.sql_max = total.sql_max.max(observation.count);
        total.unknown += observation.unknown;
        *total.status.entry(observation.status).or_default() += 1;
        total
            .duration
            .record(observation.started.elapsed().as_secs_f64());
        merge_timing(&mut total.acquisition, &observation.acquisition);
        for (id, q) in observation.queries {
            let entry = total.queries.entry(id).or_default();
            merge_timing(&mut entry.timing, &q.timing);
            entry.returned += q.returned;
            entry.affected += q.affected;
        }
    }
}
fn merge_timing(target: &mut Timing, value: &Timing) {
    target.count += value.count;
    target.total_ns = target.total_ns.saturating_add(value.total_ns);
    target.max_ns = target.max_ns.max(value.max_ns);
    for (a, b) in target.buckets.iter_mut().zip(value.buckets) {
        *a += b;
    }
}

/// Positive schema: unknown plan fields are rejected; expression values are deliberately removed.
pub fn safe_plan(value: &Value) -> Result<Value, &'static str> {
    const TEXT: &[&str] = &[
        "Node Type",
        "Parent Relationship",
        "Scan Direction",
        "Join Type",
        "Strategy",
        "Partial Mode",
        "Aggregate Mode",
        "Operation",
        "Sort Method",
        "Sort Space Type",
        "Cache Mode",
    ];
    const IDS: &[&str] = &["Relation Name", "Alias", "Index Name", "CTE Name", "Schema"];
    const DROP: &[&str] = &[
        "Filter",
        "Index Cond",
        "Recheck Cond",
        "Hash Cond",
        "Join Filter",
        "Merge Cond",
        "One-Time Filter",
        "Output",
        "Sort Key",
        "Group Key",
        "Cache Key",
        "Conflict Resolution",
        "Conflict Arbiter Indexes",
        "Subplan Name",
    ];
    const NUM: &[&str] = &[
        "Startup Cost",
        "Total Cost",
        "Plan Rows",
        "Plan Width",
        "Actual Startup Time",
        "Actual Total Time",
        "Actual Rows",
        "Actual Loops",
        "Rows Removed by Filter",
        "Rows Removed by Index Recheck",
        "Rows Removed by Join Filter",
        "Heap Fetches",
        "Shared Hit Blocks",
        "Shared Read Blocks",
        "Shared Dirtied Blocks",
        "Shared Written Blocks",
        "Local Hit Blocks",
        "Local Read Blocks",
        "Local Dirtied Blocks",
        "Local Written Blocks",
        "Temp Read Blocks",
        "Temp Written Blocks",
        "Exact Heap Blocks",
        "Lossy Heap Blocks",
        "Planning Time",
        "Execution Time",
        "Workers Planned",
        "Workers Launched",
        "I/O Read Time",
        "I/O Write Time",
        "Peak Memory Usage",
        "Hash Batches",
        "Hash Buckets",
        "Original Hash Batches",
        "Original Hash Buckets",
        "Sort Space Used",
        "Cache Hits",
        "Cache Misses",
        "Cache Evictions",
        "Cache Overflows",
        "WAL Records",
        "WAL FPI",
        "WAL Bytes",
    ];
    const BOOL: &[&str] = &[
        "Parallel Aware",
        "Async Capable",
        "Inner Unique",
        "Single Copy",
    ];
    match value {
        Value::Array(items) => items
            .iter()
            .map(safe_plan)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(object) => {
            let mut result = serde_json::Map::new();
            for (key, value) in object {
                if DROP.contains(&key.as_str()) {
                    continue;
                }
                if matches!(key.as_str(), "Plan" | "Plans" | "Planning") {
                    result.insert(key.clone(), safe_plan(value)?);
                } else if key == "Triggers" {
                    if value.as_array().is_none_or(|v| !v.is_empty()) {
                        return Err("plan trigger data rejected");
                    }
                    result.insert(key.clone(), json!([]));
                } else if NUM.contains(&key.as_str()) {
                    if !value.is_number() {
                        return Err("plan number rejected");
                    }
                    result.insert(key.clone(), value.clone());
                } else if BOOL.contains(&key.as_str()) {
                    if !value.is_boolean() {
                        return Err("plan boolean rejected");
                    }
                    result.insert(key.clone(), value.clone());
                } else if TEXT.contains(&key.as_str()) {
                    let text = value.as_str().ok_or("plan text rejected")?;
                    if text.len() > 80
                        || !text
                            .chars()
                            .all(|ch| ch.is_ascii_alphabetic() || " -".contains(ch))
                    {
                        return Err("plan label rejected");
                    }
                    result.insert(key.clone(), value.clone());
                } else if IDS.contains(&key.as_str()) {
                    let text = value.as_str().ok_or("plan identifier rejected")?;
                    if text.len() > 63
                        || !text
                            .chars()
                            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                    {
                        return Err("plan identifier rejected");
                    }
                    if key != "Schema" {
                        result.insert(key.clone(), value.clone());
                    }
                } else {
                    if key.len() <= 60
                        && key.chars().all(|ch| ch.is_ascii_alphabetic() || ch == ' ')
                    {
                        eprintln!("Unknown EXPLAIN field name (value suppressed): {key}");
                    }
                    return Err("unknown plan field rejected");
                }
            }
            Ok(Value::Object(result))
        }
        _ => Err("plan structure rejected"),
    }
}

#[test]
fn exact_sql_shapes_preserve_security_predicates() {
    let sql = source_sql("tokens", "introspect", "SELECT t.kind").unwrap_or_default();
    assert!(
        sql.contains("t.token_hash=$1")
            && sql.contains("c.secret_hash=$3")
            && sql.contains("s.credential_version=u.credential_version")
            && sql.contains("t.consumed_at IS NULL")
    );
    let me = source_sql("sessions", "me", "SELECT u.id").unwrap_or_default();
    assert!(me.contains("passkey_count") && me.contains("s.token_hash=$1"));
}
#[test]
fn plan_redaction_removes_parameter_values_and_rejects_unknown_fields() {
    let safe=safe_plan(&json!([{"Plan":{"Node Type":"Index Scan","Relation Name":"users","Index Cond":"email = 'private@example.test'","Filter":"token_hash='sensitive'","Actual Rows":1},"Execution Time":0.3}])).unwrap_or_default();
    let text = safe.to_string();
    assert!(
        !text.contains("private@example")
            && !text.contains("sensitive")
            && !text.contains("Index Cond")
    );
    assert!(safe_plan(&json!({"New Field":"secret"})).is_err());
}

#[test]
fn completed_query_events_are_counted_per_request_without_parameters() {
    use tracing::subscriber::with_default;
    use tracing_subscriber::prelude::*;
    let observer = Observer::new();
    let subscriber = tracing_subscriber::registry().with(observer.clone());
    let sql =
        source_sql("tokens", "authenticate_client", "SELECT id,client_id").unwrap_or_default();
    with_default(subscriber, || {
        let span =
            tracing::info_span!("t21_endpoint", endpoint = "introspection", status = 200_u64);
        {
            let _guard = span.enter();
            tracing::debug!(target:"sqlx::query",summary="SELECT id,client_id,secret_hash FROM",db.statement=sql.as_str(),elapsed_secs=0.002,rows_returned=1_u64,rows_affected=0_u64);
            tracing::debug!(target:"sqlx::pool::acquire",acquired_after_secs=0.001);
        }
        drop(span);
    });
    let snapshot = observer.snapshot();
    assert_eq!(snapshot["endpoints"]["introspection"]["requests"], 1);
    assert_eq!(
        snapshot["endpoints"]["introspection"]["sql_per_request_max"],
        1
    );
    assert_eq!(
        snapshot["endpoints"]["introspection"]["unknown_statement_count"],
        0
    );
    assert_eq!(
        snapshot["endpoints"]["introspection"]["pool_acquisition"]["count"],
        1
    );
    assert!(!snapshot.to_string().contains("binding-value"));
}
#[test]
fn missing_elapsed_and_unknown_statement_are_invalid_observations() {
    use tracing::subscriber::with_default;
    use tracing_subscriber::prelude::*;
    let observer = Observer::new();
    let subscriber = tracing_subscriber::registry().with(observer.clone());
    with_default(subscriber, || {
        let span = tracing::info_span!("t21_endpoint", endpoint = "account", status = 200_u64);
        {
            let _guard = span.enter();
            tracing::debug!(target:"sqlx::query",summary="SELECT private_fixture_value",db.statement="SELECT private_fixture_value",rows_returned=1_u64);
        }
        drop(span);
    });
    let snapshot = observer.snapshot();
    assert!(
        snapshot["endpoints"]["account"]["unknown_statement_count"]
            .as_u64()
            .unwrap_or(0)
            >= 1
    );
    assert!(!snapshot.to_string().contains("private_fixture_value"));
}
