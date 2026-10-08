//! Bounded operational metrics. No identity, IP, token, URL query or client ID labels.
use crate::accounts::AuthAppState;
use axum::{
    Router,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use identity_core::security::{PASSWORD_WAIT_BUCKET_NANOSECONDS, PasswordMetricsSnapshot};
use identity_store::PgPool;
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Default)]
pub(crate) struct HttpMetrics {
    pub requests: AtomicU64,
    pub failures: AtomicU64,
    pub limited: AtomicU64,
    pub nanoseconds: AtomicU64,
    buckets: [AtomicU64; 7],
}
impl HttpMetrics {
    pub fn record(&self, status: StatusCode, elapsed: std::time::Duration) {
        self.requests.fetch_add(1, Ordering::Relaxed);
        if status.is_server_error() {
            self.failures.fetch_add(1, Ordering::Relaxed);
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            self.limited.fetch_add(1, Ordering::Relaxed);
        }
        self.nanoseconds.fetch_add(
            u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        for (bucket, limit) in
            self.buckets
                .iter()
                .zip([0.005, 0.025, 0.1, 0.25, 1.0, 5.0, f64::INFINITY])
        {
            if elapsed.as_secs_f64() <= limit {
                bucket.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    fn render(&self) -> String {
        let mut result = format!(
            "identity_http_requests_total {}\nidentity_http_server_errors_total {}\nidentity_http_rate_limited_total {}\nidentity_http_duration_seconds_sum {}\nidentity_http_duration_seconds_count {}\n",
            self.requests.load(Ordering::Relaxed),
            self.failures.load(Ordering::Relaxed),
            self.limited.load(Ordering::Relaxed),
            self.nanoseconds.load(Ordering::Relaxed) as f64 / 1_000_000_000.0,
            self.requests.load(Ordering::Relaxed)
        );
        for (bucket, limit) in self
            .buckets
            .iter()
            .zip(["0.005", "0.025", "0.1", "0.25", "1", "5", "+Inf"])
        {
            result.push_str(&format!(
                "identity_http_duration_seconds_bucket{{le=\"{limit}\"}} {}\n",
                bucket.load(Ordering::Relaxed)
            ));
        }
        result
    }
}
#[derive(Clone)]
struct MetricsState {
    http: Arc<HttpMetrics>,
    pool: PgPool,
    passwords: identity_core::security::PasswordService,
    token: Option<[u8; 32]>,
}
pub fn metrics_routes(state: &AuthAppState) -> Router {
    Router::new()
        .route("/metrics", get(metrics))
        .with_state(MetricsState {
            http: state.security.http_metrics(),
            pool: state.inner.pool.clone(),
            passwords: state.inner.passwords.clone(),
            token: state.inner.metrics_token,
        })
}
async fn metrics(
    State(state): State<MetricsState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let authorization = headers.get_all("authorization").iter().collect::<Vec<_>>();
    if !identity_core::observability::metrics_authorized(
        peer.ip(),
        authorization.first().and_then(|value| value.to_str().ok()),
        authorization.len() > 1,
        state.token.as_ref(),
    ) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut output = state.http.render();
    output.push_str(&password_metrics(state.passwords.metrics()));
    output.push_str(&format!(
        "identity_database_pool_size {}\nidentity_database_pool_idle {}\n",
        state.pool.size(),
        state.pool.num_idle()
    ));
    (
        [
            ("content-type", "text/plain; version=0.0.4; charset=utf-8"),
            ("cache-control", "no-store"),
        ],
        output,
    )
        .into_response()
}
fn password_metrics(m: PasswordMetricsSnapshot) -> String {
    let mut output = format!(
        "identity_password_hashes_total {}\nidentity_password_verifications_total {}\nidentity_password_queue_timeouts_total {}\nidentity_password_hash_seconds_sum {}\nidentity_password_verification_seconds_sum {}\nidentity_argon2_memory_kib {}\nidentity_argon2_iterations {}\nidentity_argon2_lanes {}\n",
        m.hashes,
        m.verifications,
        m.queue_timeouts,
        m.hash_nanoseconds as f64 / 1_000_000_000.0,
        m.verification_nanoseconds as f64 / 1_000_000_000.0,
        m.memory_kib,
        m.iterations,
        m.lanes
    );
    output.push_str(&format!(
        "identity_password_waiting {}\nidentity_password_waiting_high_watermark {}\nidentity_password_slots_in_use {}\nidentity_password_slots_high_watermark {}\nidentity_password_running {}\nidentity_password_running_high_watermark {}\n",
        m.waiting, m.waiting_high_watermark, m.slots_in_use, m.slots_high_watermark, m.running, m.running_high_watermark
    ));
    output.push_str("# TYPE identity_password_queue_wait_seconds histogram\n");
    for (index, upper) in PASSWORD_WAIT_BUCKET_NANOSECONDS.iter().enumerate() {
        output.push_str(&format!(
            "identity_password_queue_wait_seconds_bucket{{le=\"{}\"}} {}\n",
            *upper as f64 / 1_000_000_000.0,
            m.queue_wait_buckets[index]
        ));
    }
    output.push_str(&format!(
        "identity_password_queue_wait_seconds_bucket{{le=\"+Inf\"}} {}\nidentity_password_queue_wait_seconds_count {}\nidentity_password_queue_wait_seconds_sum {}\n",
        m.queue_wait_buckets[7], m.queue_wait_buckets[7], m.queue_wait_nanoseconds as f64 / 1_000_000_000.0
    ));
    output
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_metrics_never_add_identity_labels() {
        let metrics = HttpMetrics::default();
        metrics.record(
            StatusCode::SERVICE_UNAVAILABLE,
            std::time::Duration::from_millis(10),
        );
        metrics.record(
            StatusCode::TOO_MANY_REQUESTS,
            std::time::Duration::from_millis(20),
        );
        let rendered = metrics.render();
        assert!(rendered.contains("identity_http_requests_total 2"));
        assert!(rendered.contains("identity_http_server_errors_total 1"));
        assert!(rendered.contains("le=\"+Inf\"} 2"));
        assert!(!rendered.contains("user_id"));
    }
}
