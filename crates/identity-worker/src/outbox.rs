//! Claims commit before SMTP. Only job IDs and fixed result names may be logged.
use identity_core::{
    clock::Clock,
    config::{Config, Environment, SmtpTls},
    security::{AeadEnvelope, AeadKeyRing},
};
use identity_store::{
    accounts::VerificationMail,
    passwords::{ResetMail, SecurityNotificationMail},
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
        let content = match lease.template.as_str() {
            "verify_email" => {
                let payload: VerificationMail =
                    serde_json::from_slice(&plaintext).map_err(|_| WorkerError::InvalidPayload)?;
                self.link_content(
                    ("验证您的邮箱", "前往验证邮箱"),
                    &payload.verification_url,
                    &payload.expires_at,
                    "/email-verification",
                    Duration::minutes(30),
                    lease,
                )?
            }
            "reset_password" => {
                let payload: ResetMail =
                    serde_json::from_slice(&plaintext).map_err(|_| WorkerError::InvalidPayload)?;
                self.link_content(
                    ("重置您的密码", "前往重置密码"),
                    &payload.reset_url,
                    &payload.expires_at,
                    "/password-reset",
                    Duration::minutes(15),
                    lease,
                )?
            }
            "security_notification" => {
                let payload: SecurityNotificationMail =
                    serde_json::from_slice(&plaintext).map_err(|_| WorkerError::InvalidPayload)?;
                let description = match payload.event.as_str() {
                    "password.changed" => "您的账号密码已修改。",
                    "password.reset_completed" => "您的账号密码已通过邮箱找回完成重置。",
                    "mfa.totp_enrolled" => "您的账号已开启身份验证器双重验证。",
                    "mfa.totp_removed" => "您的账号身份验证器双重验证已关闭。",
                    "mfa.recovery_codes_regenerated" => {
                        "您的账号恢复码已重新生成，旧恢复码已失效。"
                    }
                    "passkey.registered" => "您的账号已添加新的通行密钥。",
                    "passkey.renamed" => "您的账号通行密钥名称已修改。",
                    "passkey.removed" => "您的账号通行密钥已删除。",
                    _ => return Err(WorkerError::InvalidPayload),
                };
                let occurred = OffsetDateTime::parse(&payload.occurred_at, &Rfc3339)
                    .map_err(|_| WorkerError::InvalidPayload)?;
                if occurred > self.clock.now() {
                    return Err(WorkerError::InvalidPayload);
                }
                let text = Zeroizing::new(format!(
                    "账号安全通知\n\n{description}\n操作时间（UTC）：{}。\n原有设备会话及应用授权已退出，已绑定的身份验证器与通行密钥保持不变。\n如果不是您本人操作，请通过身份中心的官方入口重新找回密码，并检查您的邮箱安全。\n本邮件不包含密码，请勿回复认证秘密。\n",
                    payload.occurred_at
                ));
                let html = Zeroizing::new(format!(
                    "<!doctype html><html lang=\"zh-CN\"><body><h1>账号安全通知</h1><p>{description}</p><p>操作时间（UTC）：{}。</p><p>原有设备会话及应用授权已退出，已绑定的身份验证器与通行密钥保持不变。</p><p>如果不是您本人操作，请通过身份中心的官方入口重新找回密码，并检查邮箱安全。</p></body></html>",
                    escape_html(&payload.occurred_at)
                ));
                MailContent {
                    subject: "统一身份中心 — 账号安全通知",
                    text,
                    html,
                }
            }
            _ => return Err(WorkerError::InvalidPayload),
        };
        let recipient: Mailbox = lease
            .recipient
            .parse()
            .map_err(|_| WorkerError::InvalidPayload)?;
        Message::builder()
            .from(self.from.clone())
            .to(recipient)
            .subject(content.subject)
            .message_id(Some(format!(
                "<{}.identity-outbox@{}>",
                lease.id,
                self.issuer.host_str().ok_or(WorkerError::Configuration)?
            )))
            .multipart(
                MultiPart::alternative()
                    .singlepart(SinglePart::plain(content.text.to_string()))
                    .singlepart(SinglePart::html(content.html.to_string())),
            )
            .map_err(|_| WorkerError::InvalidPayload)
    }

    fn link_content(
        &self,
        labels: (&str, &str),
        url: &str,
        expires_at: &str,
        path: &str,
        lifetime: Duration,
        lease: &OutboxLease,
    ) -> Result<MailContent, WorkerError> {
        let (title, button) = labels;
        validate_action_link(&self.issuer, url, path)?;
        let expires =
            OffsetDateTime::parse(expires_at, &Rfc3339).map_err(|_| WorkerError::InvalidPayload)?;
        if expires <= self.clock.now() || expires > lease.lease_until + lifetime {
            return Err(WorkerError::InvalidPayload);
        }
        let subject = match path {
            "/email-verification" => "统一身份中心 — 验证您的邮箱",
            "/password-reset" => "统一身份中心 — 重置您的密码",
            _ => return Err(WorkerError::InvalidPayload),
        };
        Ok(MailContent {
            subject,
            text: Zeroizing::new(format!(
                "{title}\n\n请打开以下链接，再点击页面上的确认按钮完成操作：\n{url}\n\n链接到期时间（UTC）：{expires_at}。打开链接本身不会消费链接。\n如果这不是您发起的操作，请忽略此邮件。请勿将链接转发给他人。\n"
            )),
            html: Zeroizing::new(format!(
                "<!doctype html><html lang=\"zh-CN\"><body><h1>{title}</h1><p>请打开链接，再点击页面上的确认按钮完成操作。</p><p><a href=\"{}\">{button}</a></p><p>链接到期时间（UTC）：{}。打开链接本身不会消费链接。</p><p>如果这不是您发起的操作，请忽略此邮件。请勿转发链接。</p></body></html>",
                escape_html(url),
                escape_html(expires_at)
            )),
        })
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
#[cfg(test)]
fn validate_verification_link(issuer: &Url, text: &str) -> Result<(), WorkerError> {
    validate_action_link(issuer, text, "/email-verification")
}
fn validate_action_link(issuer: &Url, text: &str, path: &str) -> Result<(), WorkerError> {
    let url = Url::parse(text).map_err(|_| WorkerError::InvalidPayload)?;
    if url.origin() != issuer.origin()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != path
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
struct MailContent {
    subject: &'static str,
    text: Zeroizing<String>,
    html: Zeroizing<String>,
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
