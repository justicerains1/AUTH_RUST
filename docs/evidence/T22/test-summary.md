# T22 单机生产制品与本地运维验证

2026-10-08，父75a93dc。production部署、独立SMTP/备份/告警未提供，原T20/T21前置未全验收，因此T22整体保持待验收；本次提交的是已测试制品、指标、加密维护与本地演练。

多阶段Rust/Caddy+三前端镜像真实构建，固定digest/精确runtime包，UID10001/read-only/tmpfs/资源限制、仅edge80/443、PG/Redis无外端口、秘密只受限文件。Caddy真配置validate0、应用镜像readonly检查0。镜像digest对应构建时快照，后续代码/指标/维护新增后正式发布需重新构建，不当最终生产版本。

API/Worker内部/metrics已实现Bearer受限tokenfile（可选未配置只loopback），固定聚合指标/HTTPhistogram/Argon2/连接池/真实outbox队列，无身份秘密标签。核心授权/histogram单元及真实testschemaAPI200、401计数/Workerpending0通过。Promtool3.15.0官方digest核验，9条规则/配置语法0；没有实际生产采集或告警送达。

age1.3.2来源/摘要/许可核对，WAL归档加密/幂等/拒覆盖/拒损坏/atomicpublish与restore边界实测。两个专属临时PG容器真实basebackup/verifybackup、age加密/WAL归档、PITR至namedpoint：保留前两行、排除之后第三行，1.903s，小样本不代生产RPO/RTO，资源只清自己。

密钥维护CLI public仅公钥、reencrypt要求版本map+明确生产ack，按准确user/purpose/namespace AAD批量事务。真实integration1case0ignored0：TOTP/outbox/4挑战/BFFflow/session旧新重包裹，原文一致、真实TOTP仍匹配、损坏tagrollback、再次0改写；actualCLI公共导出与参数幂等执行通过。完整runnerT22必须执行metrics和keyintegration，缺环境失败不skipped。

root最终check/unit/build/docs/openapi/tooling0，身份59+演示12前端测试；代码无未绑定占位首页，保留历史证据。恢复尚未账户/TOTP/Passkey/撤销全链生产验证，每日14天保留/WAL独立故障域/磁盘证书备份告警/DNS邮件送达与签名真实轮换仍是待验收，不发布。

最终审计修复了基础备份迁移路径校验、生产镜像缺失 `identity-keys`、WAL 切段及独立目录映射，详见 [运维修复](../T23/ops-repair-summary.md)。`npm run test:integration -- --task=T22` 现必须执行真实指标、AEAD 密钥维护和 12 项备份/WAL/配置检查，缺工具失败；2026-10-08T12:33:50Z 三阶段均实际退出 0，根 `npm run check` 退出 0。镜像重建和最终全量回归另记录，不能用此局部结果释放生产关卡。


2026-10-09本地补充已实现并实测：真实签名预发布/切签/缓存刷新/回滚/期限[演练](signing-rotation-summary.md)、完整密码/TOTP/虚拟Passkey及live/revokedOAuth[恢复](identity-restore-summary.md)、18总规则/20场景及真实磁盘TLS备份采集[告警](host-metrics/test-summary.md)、实际备份→迁移中止→启动/兼容回滚[发布](release-orchestration-summary.md)。T22入口现在六组必跑，告警/恢复不只存在脚本；正式SMTP、独立主机/存储/14天调度/可达通知/实际生产RPO与RTO仍待外部验收。最终当前全量报告和镜像另行更新。

## 本轮WAL与监控链补验

[WAL监测](wal-monitoring/test-summary.md)已实现预建expecteddevice受控归档根、durable completion回执/原子指针、重试不刷新且任一步sync失败不返回成功、真实pg_stat_archiver失败/恢复/权限拒绝指标；38运维项、25规则/31场景通过。

[本地监控链](monitoring-chain/test-summary.md)实际collector→node_exporter→Prometheus→Alertmanager→回环receiver，原for:1m后firing及恢复resolved已送达；新增受控collector systemd service/timer与私网Prometheus接入配置。生产调度/接收方、独立故障域/十四天历史/RPO-RTO仍待真实环境。T22入口现含完整链必跑，最终整个T22/全量结果后更新；五服务发布/回滚新演练正在实现，不能沿用只API范围冒整套通过。
