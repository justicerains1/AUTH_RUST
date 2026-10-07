//! Claims commit before SMTP. Only job IDs and fixed result names may be logged.
use identity_core::{
    clock::Clock,
    config::{Config, Environment, SmtpTls},
    security::{AeadEnvelope, AeadKeyRing},
};
use identity_store::{
    accounts::VerificationMail,
    repository::{OutboxLease, Repository},
};
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Mailbox, MultiPart, SinglePart},
    transport::smtp::authentication::Credentials,
};
use sqlx::PgPool;
use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration as StdDuration,
};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use url::Url;
use uuid::Uuid;
use zeroize::Zeroizing;

const RETRY_MINUTES: [i64; 5] = [1, 5, 15, 60, 180];
const SMTP_TIMEOUT: StdDuration = StdDuration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerError {
    Configuration,
    DatabaseUnavailable,
    LeaseConflict,
    InvalidPayload,
    SmtpUnavailable,
}
impl fmt::Display for WorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Configuration => "worker configuration invalid",
            Self::DatabaseUnavailable => "worker database unavailable",
            Self::LeaseConflict => "worker lease conflict",
            Self::InvalidPayload => "worker encrypted payload invalid",
            Self::SmtpUnavailable => "SMTP delivery unavailable",
        })
    }
}
impl std::error::Error for WorkerError {}

#[derive(Default)]
struct WorkerMetrics {
    delivered: AtomicU64,
    retried: AtomicU64,
    failed: AtomicU64,
    database_failures: AtomicU64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BatchResult {
    pub claimed: u64,
    pub delivered: u64,
    pub retried: u64,
    pub failed: u64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorkerMetricsSnapshot {
    pub delivered: u64,
    pub retried: u64,
    pub failed: u64,
    pub database_failures: u64,
}

pub struct MailWorker {
    repository: Repository,
    pool: PgPool,
    clock: Arc<dyn Clock>,
    keys: AeadKeyRing,
    smtp: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
    issuer: Url,
    worker_id: Uuid,
    metrics: WorkerMetrics,
}
impl MailWorker {
    pub fn new(config: &Config, pool: PgPool, clock: Arc<dyn Clock>) -> Result<Self, WorkerError> {
        let builder = match config.smtp_tls {
            SmtpTls::Required => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.smtp_host)
                    .map_err(|_| WorkerError::Configuration)?
            }
            SmtpTls::DisabledForDevelopment => {
                if config.environment == Environment::Production
                    || !matches!(
                        config.smtp_host.as_str(),
                        "localhost" | "127.0.0.1" | "::1" | "mailpit"
                    )
                {
                    return Err(WorkerError::Configuration);
                }
                AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.smtp_host)
            }
        };
        let mut builder = builder.port(config.smtp_port).timeout(Some(SMTP_TIMEOUT));
        if let (Some(username), Some(password)) = (&config.smtp_username, &config.smtp_password) {
            builder = builder.credentials(Credentials::new(
                username.expose().to_owned(),
                password.expose().trim_end().to_owned(),
            ));
        }
        let keys =
            AeadKeyRing::load_file(&config.encryption_keys_file, &config.active_encryption_kid)
                .map_err(|_| WorkerError::Configuration)?;
        let from = config
            .smtp_from
            .parse()
            .map_err(|_| WorkerError::Configuration)?;
        Ok(Self {
            repository: Repository::new(pool.clone(), clock.clone()),
            pool,
            clock,
            keys,
            smtp: builder.build(),
            from,
            issuer: config.issuer.clone(),
            worker_id: Uuid::new_v4(),
            metrics: WorkerMetrics::default(),
        })
    }

    pub async fn run_once(&self) -> Result<BatchResult, WorkerError> {
        let leases = self
            .repository
            .claim_outbox(self.worker_id, 10, Duration::minutes(5))
            .await
            .map_err(|_| {
                self.metrics
                    .database_failures
                    .fetch_add(1, Ordering::Relaxed);
                WorkerError::DatabaseUnavailable
            })?;
        let mut batch = BatchResult {
            claimed: leases.len() as u64,
            ..BatchResult::default()
        };
        for lease in leases {
            let result = self.message(&lease);
            let sent = match result {
                Ok(message) => {
                    match tokio::time::timeout(SMTP_TIMEOUT, self.smtp.send(message)).await {
                        Ok(Ok(_)) => Ok(()),
                        _ => Err(WorkerError::SmtpUnavailable),
                    }
                }
                Err(error) => Err(error),
            };
            match sent {
                Ok(()) => {
                    self.delivery_committed(&lease).await?;
                    batch.delivered += 1;
                    self.metrics.delivered.fetch_add(1, Ordering::Relaxed);
                    tracing::info!(outbox_id=%lease.id,result="delivered");
                }
                Err(error) => {
                    let retry = self
                        .delivery_failed(&lease, error == WorkerError::InvalidPayload)
                        .await?;
                    if retry {
                        batch.retried += 1;
                        self.metrics.retried.fetch_add(1, Ordering::Relaxed);
                    } else {
                        batch.failed += 1;
                        self.metrics.failed.fetch_add(1, Ordering::Relaxed);
                    }
                    tracing::warn!(outbox_id=%lease.id,result=if retry {"retry_scheduled"} else {"permanent_failure"});
                }
            }
        }
        Ok(batch)
    }

    fn message(&self, lease: &OutboxLease) -> Result<Message, WorkerError> {
        let user_id = lease.user_id.ok_or(WorkerError::InvalidPayload)?;
        let envelope: AeadEnvelope = serde_json::from_value(lease.encrypted_params.0.clone())
            .map_err(|_| WorkerError::InvalidPayload)?;
        let plaintext = self
            .keys
            .decrypt(user_id, "email-outbox", &envelope)
            .map_err(|_| WorkerError::InvalidPayload)?;
        if lease.template != "verify_email" {
            return Err(WorkerError::InvalidPayload);
        }
        let payload: VerificationMail =
            serde_json::from_slice(&plaintext).map_err(|_| WorkerError::InvalidPayload)?;
        validate_verification_link(&self.issuer, &payload.verification_url)?;
        let expires = OffsetDateTime::parse(&payload.expires_at, &Rfc3339)
            .map_err(|_| WorkerError::InvalidPayload)?;
        let now = self.clock.now();
        if expires <= now || expires > lease.lease_until + Duration::minutes(30) {
            return Err(WorkerError::InvalidPayload);
        }
        let recipient: Mailbox = lease
            .recipient
            .parse()
            .map_err(|_| WorkerError::InvalidPayload)?;
        let text = Zeroizing::new(format!(
            "验证您的邮箱\n\n请打开以下链接，再点击页面上的确认按钮完成邮箱验证：\n{}\n\n链接到期时间（UTC）：{}。打开链接本身不会完成验证。\n如果这不是您发起的注册，请忽略此邮件。请勿将链接转发给他人。\n",
            payload.verification_url, payload.expires_at
        ));
        let html = Zeroizing::new(format!(
            "<!doctype html><html lang=\"zh-CN\"><body><h1>验证您的邮箱</h1><p>请打开链接，再点击页面上的确认按钮完成邮箱验证。</p><p><a href=\"{}\">前往验证邮箱</a></p><p>链接到期时间（UTC）：{}。打开链接本身不会完成验证。</p><p>如果这不是您发起的注册，请忽略此邮件。请勿转发链接。</p></body></html>",
            escape_html(&payload.verification_url),
            escape_html(&payload.expires_at)
        ));
        Message::builder()
            .from(self.from.clone())
            .to(recipient)
            .subject("统一身份中心 — 验证您的邮箱")
            .message_id(Some(format!(
                "<{}.identity-outbox@{}>",
                lease.id,
                self.issuer.host_str().ok_or(WorkerError::Configuration)?
            )))
            .multipart(
                MultiPart::alternative()
                    .singlepart(SinglePart::plain(text.to_string()))
                    .singlepart(SinglePart::html(html.to_string())),
            )
            .map_err(|_| WorkerError::InvalidPayload)
    }

    async fn delivery_committed(&self, lease: &OutboxLease) -> Result<(), WorkerError> {
        let now = self.clock.now();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| WorkerError::DatabaseUnavailable)?;
        let result=sqlx::query("UPDATE email_outbox SET state='delivered',delivered_at=$3,encrypted_params=NULL,lease_id=NULL,lease_until=NULL WHERE id=$1 AND lease_id=$2 AND lease_until>$3 AND state='pending'").bind(lease.id).bind(lease.owner).bind(now).execute(&mut *tx).await.map_err(|_|WorkerError::DatabaseUnavailable)?;
        if result.rows_affected() != 1 {
            return Err(WorkerError::LeaseConflict);
        }
        delivery_audit(&mut tx, lease, "email.delivery_succeeded", "success", now).await?;
        tx.commit()
            .await
            .map_err(|_| WorkerError::DatabaseUnavailable)
    }
    async fn delivery_failed(
        &self,
        lease: &OutboxLease,
        invalid_payload: bool,
    ) -> Result<bool, WorkerError> {
        let now = self.clock.now();
        let retry = if invalid_payload {
            None
        } else {
            retry_delay(lease.attempts)
        };
        let next = retry.map(|delay| now + delay);
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| WorkerError::DatabaseUnavailable)?;
        let result=sqlx::query("UPDATE email_outbox SET state=CASE WHEN $4::timestamptz IS NULL THEN 'failed' ELSE 'pending' END,next_attempt_at=COALESCE($4,next_attempt_at),failed_at=CASE WHEN $4::timestamptz IS NULL THEN $3 ELSE NULL END,lease_id=NULL,lease_until=NULL WHERE id=$1 AND lease_id=$2 AND lease_until>$3 AND state='pending'").bind(lease.id).bind(lease.owner).bind(now).bind(next).execute(&mut *tx).await.map_err(|_|WorkerError::DatabaseUnavailable)?;
        if result.rows_affected() != 1 {
            return Err(WorkerError::LeaseConflict);
        }
        delivery_audit(&mut tx, lease, "email.delivery_failed", "failure", now).await?;
        tx.commit()
            .await
            .map_err(|_| WorkerError::DatabaseUnavailable)?;
        Ok(retry.is_some())
    }
    pub async fn ready(&self) -> bool {
        sqlx::query("SELECT 1").execute(&self.pool).await.is_ok()
    }
    pub fn metrics(&self) -> WorkerMetricsSnapshot {
        WorkerMetricsSnapshot {
            delivered: self.metrics.delivered.load(Ordering::Relaxed),
            retried: self.metrics.retried.load(Ordering::Relaxed),
            failed: self.metrics.failed.load(Ordering::Relaxed),
            database_failures: self.metrics.database_failures.load(Ordering::Relaxed),
        }
    }
}
async fn delivery_audit(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    lease: &OutboxLease,
    event: &str,
    result: &str,
    now: OffsetDateTime,
) -> Result<(), WorkerError> {
    sqlx::query("INSERT INTO audit_events(id,event,actor_id,target_type,target_id,result,request_id,source,occurred_at) VALUES($1,$2,NULL,'outbox',$3,$4,$5,'worker',$6)").bind(Uuid::new_v4()).bind(event).bind(lease.id).bind(result).bind(Uuid::new_v4()).bind(now).execute(&mut **tx).await.map_err(|_|WorkerError::DatabaseUnavailable)?;
    Ok(())
}
pub fn retry_delay(attempts: i32) -> Option<Duration> {
    usize::try_from(attempts - 1)
        .ok()
        .and_then(|index| RETRY_MINUTES.get(index))
        .map(|minutes| Duration::minutes(*minutes))
}
fn validate_verification_link(issuer: &Url, text: &str) -> Result<(), WorkerError> {
    let url = Url::parse(text).map_err(|_| WorkerError::InvalidPayload)?;
    if url.origin() != issuer.origin()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/email-verification"
        || url.query().is_some()
    {
        return Err(WorkerError::InvalidPayload);
    }
    let token = url
        .fragment()
        .and_then(|fragment| fragment.strip_prefix("token="))
        .ok_or(WorkerError::InvalidPayload)?;
    if token.len() != 43
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
    {
        return Err(WorkerError::InvalidPayload);
    }
    Ok(())
}
fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_schedule_is_bounded_and_exact() {
        for (index, minutes) in [1, 5, 15, 60, 180].into_iter().enumerate() {
            assert_eq!(
                retry_delay(index as i32 + 1),
                Some(Duration::minutes(minutes))
            );
        }
        assert_eq!(retry_delay(0), None);
        assert_eq!(retry_delay(6), None);
    }
    #[test]
    fn verification_links_reject_different_origin_query_or_nonfragment_tokens()
    -> Result<(), Box<dyn std::error::Error>> {
        let issuer = Url::parse("https://identity.example")?;
        let token = "a".repeat(43);
        assert!(
            validate_verification_link(
                &issuer,
                &format!("https://identity.example/email-verification#token={token}")
            )
            .is_ok()
        );
        for url in [
            format!("https://evil.example/email-verification#token={token}"),
            format!("https://identity.example/email-verification?token={token}"),
            format!("https://identity.example/other#token={token}"),
            "https://identity.example/email-verification#token=short".into(),
        ] {
            assert!(validate_verification_link(&issuer, &url).is_err());
        }
        assert_eq!(escape_html("<&\"'"), "&lt;&amp;&quot;&#39;");
        Ok(())
    }
}
