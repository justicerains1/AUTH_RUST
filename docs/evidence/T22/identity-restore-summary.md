# T22 本地完整身份快照恢复演练

2026-10-08 UTC。新增[真实API harness](../../../crates/identity-server/tests/t22_identity_restore.rs)、[备份恢复runner](../../../tests/ops/identity-restore.mjs)与[浏览器认证器/TOTP客户端](../../../tests/ops/identity-ceremonies.mjs)。生产源码和现有备份脚本未修改；此演练扩大原小probe的业务验证范围，不冒生产独立主机/RPO/RTO。

最终实际命令`PATH=/root/.cargo/bin:$PATH node tests/ops/identity-restore.mjs`：15:01:54.111～15:02:23.968 UTC退出0，5组业务检查完成，两个真实Rust API harness均退出0。原报告[identity-restore-passed-1791471743968.json](identity-restore-passed-1791471743968.json)保存准确提交`eefeb65f`（含本次未提交的新测试）、源码摘要、密文备份摘要/ID、检查及本地计时。此前完整成功和失败均保留。定向Clippy、ESLint和diff-check退出0。

## 实际数据与认证行为

- 专属PG17.11容器`identity-recovery-<UUID>`使用network-none、无主机端口；API通过任务目录内的Unix socket连接，socket目录UID999/0700，父任务目录仅当前运维用户可访问。不同测试schema仅在专属identity_test数据库内。没有接入或停止开发PG；Redis仅读写本次随机AEAD namespace预算，不停止共享依赖。
- 初始同一已验证测试账号真实密码登录、近期密码确认后由Chromium `navigator.credentials.create`与CDP CTAP2 resident/UV认证器注册真实Passkey。再以实际新challenge/签名完成Passkey近期强认证，才允许TOTP注册；独立HMAC客户端完成六位TOTP确认。没有插入合成因素密文或手改strong_at。
- 同账号实际密码+TOTP登录，创建两受控client并通过真实authorize/同意/Basic/PKCE token交换建立两家族；一个live access/refresh权威有效，另一个实际revoke后access inactive。保留活跃session、密码hash、TOTP密文/重放counter、真实Passkey公钥、授权/令牌摘要与撤销状态于数据库。
- API停止后调用现有`base-backup.sh`实际`pg_basebackup --format=plain --wal-method=stream`、`pg_verifybackup`、tar和age1.3.2加密。只把密文/相对checksum及独立签名/AEAD/backup身份文件复制到新目录，核对key副本内容一致。随后删除专属源PG容器、源数据/密钥目录，再从新目录解密/verify并启动第二专属PG，避免从旧DB或旧key文件误读成功。
- 恢复后原活跃session/me200；live access权威active、真实refresh200，已撤家族inactive/refresh invalid_grant。恢复签名key新签ID Token通过独立jose验证RS256/JWKS/issuer/audience/sub。验证时间为该真实token签发时刻，和API可控时钟一致，不关闭exp检查。
- 退出原session后原密码+原TOTP新时间步实际登录成功。将注册时的**私有虚拟认证器凭证**仅在内存迁移到新CDP认证器，`navigator.credentials.get`获得新challenge并实际签名；恢复公钥验证同用户登录，`amr=user`、`strong_at`存在且无`hwk`虚假硬件声明。不是重放备份前proof，也不是实体认证器测试。
- 恢复后实际全设备退出204，刚刷新access下一次introspection inactive。没有因恢复丢失原撤销状态或停用普通权威检查。

本地计时从新目录开始restore-base到最后业务验证完成为**7.711秒**；数据量仅该场景账号/因素/两家族，作为本地完整业务演练耗时记录，不能推算生产RTO≤60分钟。使用已知基础快照，没有事故点/持续WAL丢失窗口，**不报告生产RPO**。原真实命名点WAL重放仍见[local-pitr-drill.md](local-pitr-drill.md)，本次不谎称另做PITR。

## 失败和秘密边界

初次真实Passkey注册返回201而测试写200、真实logout返回204而测试写200、revoke成功空body而测试强制JSON解析，均整体非零并保留`identity-restore-failure-*.txt/json`。顺序新增Passkey后TOTP绑定需要强认证，测试仅密码确认收到403，改为真实Passkey近期认证后继续；未改生产安全条件。最后测试误期望`hwk`，实际实现按文档正确`user`且无硬件证明，修改为明确user/nohwk/strong_at，整轮复跑成功。所有修正都是测试契约/场景，未删除失败或放宽生产规则。

默认age/age-keygen来自`.local/security-tools/age`核验工具，缺Docker/age/浏览器/数据库harness会失败，不能skip-green。原始harness诊断存任务0600目录，失败后替换密码、AEAD、URL、token、完整签名/age私钥及虚拟credential.privateKey再保存公共诊断。成功JSON仅检查说明、源码与密文摘要，不含账号/token/Cookie、因素种子或私钥；结束清自己的容器、数据、key目录和浏览器上下文。

root将本演练接入T22必要runner后再执行集成/完整检查。正式不同故障域存储、每日基础备份/连续WAL、14天恢复链、完整生产规模和实体Passkey、新主机事故RPO/RTO及告警仍需独立真实验收，T22整体不因此放行。
