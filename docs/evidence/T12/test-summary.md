# T12 刷新轮换、即时撤销与 RP 退出验收

2026-10-08，父提交3a06d16，Linux/Rust1.98/Node22、真实PG17/Redis7.4。模块的五个必要案例均实际通过，Git提交记录最终版本。

- refresh按user→session→grant→family锁序轮换；旧refresh串行/同步并发重放先提交家族撤销及审计，再返回invalid_grant。签名失败旧token未消费且不新增pair；token-levelscope可缩小，再扩大invalid_scope，userinfo/introspection不恢复email权限。
- introspection每次当前权威查询，跨client/未知/失效仅active=false；revoke跨client不撤，refresh撤整个grant，access至少该token；本人grant列表签名cursor及显式撤销。
- RP GET/POSTform仅建立五分钟持久确认；普通JWT仍拒过期，专用hint只准确issuer/audience/sub/sid及12h+2m兼容。坏签名/用户/sid不能授权外部回调。最终JSON POST严验Origin/CSRF/当前绑定，取消不撤；preauth确认不能升级撤后来身份，跨浏览器拒。提交时复核当前client/注册回调。
- 行锁同步证明撤销先提交则刷新失败；反向刷新后退出使新token立即无效。实际等待user锁中的logout对期间新建token使用锁后时间，避免撤销时间早于created_at。相同修复覆盖密码变化及普通退出；T06/T07真实回归通过。
- 真实停止PG和Redis容器，introspection503且没有active，finally恢复healthy；closedpool边界单独标识。成熟openid-client6.8.8执行实际refresh/introspect，ID Token不回放nonce；jose6.2.12独立签名/声明负向验证。

真实integration退出0，Playwright2passed/0failed；check、unit、build、docs、OpenAPI及65工具测试均通过，身份前端40项加A/B各2项。T03/T06/T07/T11受影响回归均0，旧证据保留。最终记录integration.txt/e2e.txt/final-check.txt/unit.txt/build.txt/docs.txt/openapi.txt/tooling.txt；细节见test-boundaries.md及[事务说明](../../revocation-transactions.md)。

失败与修复：无hint空form错误拒绝已修，测试审计字典不一致已修，默认30秒测试pool对齐生产2秒后真实故障503；最早并发异常随后加严格error/状态断言复测通过，不能以无证据旧快照猜测归因。锁前时间可能逆序违反CHECK的问题已修并新增确定竞争窗口验证。所有脱敏失败记录保留。JSONconfirm成功响应已同步OpenAPI，仍标准OAuth错误，不套平台中文错误。

CI恢复修复39f5b83后远端T03通过，下一失败位于T07E2E；Windows已实际诊断为OpenSSL开发库缺失，3a06d16配置预装3.6.4后远端验证进行中。模块本地通过不代表CI/全量T20～T23通过。生产资源未准备，真实设备待验收，未生产部署。
