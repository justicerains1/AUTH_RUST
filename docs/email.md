# 邮箱动作与真实SMTP Worker

本页对应 T05。注册/重发在服务端事务创建验证动作及outbox；Worker只负责已提交outbox的实际投递。身份/API输入契约见 [OpenAPI](api/openapi.yaml)，数据与租约见 [database.md](database.md)。

## 链接与模板

`verify_email` 加密载荷使用 `identity_store::accounts::VerificationMail { verification_url, expires_at }`；AEAD用途固定`email-outbox`并绑定outbox.user_id。链接由固定Config.ISSUER生成 `/email-verification#token=<43位base64url>`，30分钟到期。Worker解密后再次核对链接origin、路径、无query/userinfo及合法fragment token，不能发送用户输入的Host/return_to/任意外部URL。

邮件MIME为text/plain与text/html alternative，中文主题“统一身份中心 — 验证您的邮箱”，说明用途、UTC到期、打开链接本身不消费以及非本人忽略提示。HTML插值经过转义；正文不把用户名称作为HTML。Message-ID以outbox UUID组成以便至少一次重试关联，不打印邮件正文、收件人、链接、token或加密参数。

当前实现只投递T05验证模板；数据库已有reset_password/security_notification名，具体载荷与后续密码/安全通知模板在T07等任务补齐，不能把本任务称为已完成密码重置邮件。

## 数据事务与投递

1. API在同事务提交user/email_action/encrypted outbox；SMTP不在API或数据库事务中调用。
2. Worker经Repository.claim_outbox用SKIP LOCKED领取到期pending项，租约每批随机nonce、5分钟，attempts加一，领取事务先提交。
3. 解密载荷并做固定issuer验证后，lettre真实SMTP发送，连接/整个send各有限超时10秒。
4. 成功后仅当前有效lease nonce可更新delivered，清encrypted_params/lease并记录安全投递审计；提交失败保留租约，随后可能重投，符合至少一次语义。
5. SMTP失败按第一次1分钟、第二次5分钟、第三次15分钟、第四次60分钟、第五次180分钟重试；第六次失败为failed，清租约并指标增加。非法/过期载荷立即failed，不不断投递无效链接。

投递审计与outbox结果事务独立于此前账号安全操作；投递或审计失败不会回滚已经提交的注册/密码安全变化。Worker日志只含outbox UUID和固定result。worker.metrics()包含delivered/retried/failed/database_failures原子计数，无邮箱、用户或token标签，永久失败供运维告警采集。

## 配置与故障

SMTP_TLS=required使用lettre的required STARTTLS，rustls验证服务器证书和域名，不关闭校验、不降级到明文；SMTP用户名和密码由Config校验与秘密文件载入。SMTP_TLS=disabled只允许development/test且host为localhost/127.0.0.1/::1/mailpit，生产拒绝。Mailpit本地1025用于真实开发投递，不能证明生产SMTP送达或邮件DNS配置。

main运行真实循环（每次批处理后1秒可取消等待），SIGTERM/Ctrl-C同时停止HTTP和领取循环；发送中取消保留租约供后续领取，可能重投，不谎称正好一次。数据库错误保守等待下一次，未发送项不丢弃。/health/live只证明进程；/health/ready查询PG，不因SMTP暂时不可用阻断outbox重试；SMTP失败由计数/审计/队列状态呈现，不冒充邮件已送达。

真实故障验收应停止/恢复Mailpit，并检查pending/attempts/next_attempt_at、恢复后delivered及清秘密；用显式数据库时间构造重试到期，避免等待整段分钟计划。生产SMTP、SPF/DKIM/DMARC、真实告警可达由T22实际验收。
