# T22 单机生产制品与本地运维验证

2026-10-08，父75a93dc。production部署、独立SMTP/备份/告警未提供，原T20/T21前置未全验收，因此T22整体保持待验收；本次提交的是已测试制品、指标、加密维护与本地演练。

多阶段Rust/Caddy+三前端镜像真实构建，固定digest/精确runtime包，UID10001/read-only/tmpfs/资源限制、仅edge80/443、PG/Redis无外端口、秘密只受限文件。Caddy真配置validate0、应用镜像readonly检查0。镜像digest对应构建时快照，后续代码/指标/维护新增后正式发布需重新构建，不当最终生产版本。

API/Worker内部/metrics已实现Bearer受限tokenfile（可选未配置只loopback），固定聚合指标/HTTPhistogram/Argon2/连接池/真实outbox队列，无身份秘密标签。核心授权/histogram单元及真实testschemaAPI200、401计数/Workerpending0通过。Promtool3.15.0官方digest核验，9条规则/配置语法0；没有实际生产采集或告警送达。

age1.3.2来源/摘要/许可核对，WAL归档加密/幂等/拒覆盖/拒损坏/atomicpublish与restore边界实测。两个专属临时PG容器真实basebackup/verifybackup、age加密/WAL归档、PITR至namedpoint：保留前两行、排除之后第三行，1.903s，小样本不代生产RPO/RTO，资源只清自己。

密钥维护CLI public仅公钥、reencrypt要求版本map+明确生产ack，按准确user/purpose/namespace AAD批量事务。真实integration1case0ignored0：TOTP/outbox/4挑战/BFFflow/session旧新重包裹，原文一致、真实TOTP仍匹配、损坏tagrollback、再次0改写；actualCLI公共导出与参数幂等执行通过。完整runnerT22必须执行metrics和keyintegration，缺环境失败不skipped。

root最终check/unit/build/docs/openapi/tooling0，身份59+演示12前端测试；代码无未绑定占位首页，保留历史证据。恢复尚未账户/TOTP/Passkey/撤销全链生产验证，每日14天保留/WAL独立故障域/磁盘证书备份告警/DNS邮件送达与签名真实轮换仍是待验收，不发布。
