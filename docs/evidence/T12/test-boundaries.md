# T12 真实验证记录与边界

独立 PostgreSQL identity_test 临时 schema、真实 Redis、真实 Axum HTTP 与成熟 openid-client 6.8.8/jose 6.2.12。所有令牌、Cookie、签名 hint 和秘密只在内存或忽略目录 .local；未采集浏览器 trace、截图或视频。

已验证刷新原子轮换、串行及同步并发旧 refresh 重放后家族撤销与审计提交；不同客户端 introspection/revoke 隔离；未知 token/hint 安全行为；两种提交交错下撤销后无新有效凭证。受控行锁证明刷新阻塞于真实用户权威锁，撤销提交先行后刷新失败；逆序实测先刷新成功再真实退出提交，新令牌立即无效。

RP GET/POST form 只建立确认。真实有效签名、已过期且匹配当前用户/session 的 hint 可请求确认，普通 ID Token 验证仍拒过期；错误签名/audience/user/sid 被拒，无效 hint 不能授权外部回调。缺少 CSRF 的确认不能撤销；有效确认才撤销。浏览器真实授权列表撤销、退出取消与确认共 2 passed / 0 failed。

真实停止 Redis 和 PostgreSQL 容器时 introspection 都返回503且没有 active 字段；依赖在 finally 恢复并等待健康。测试连接池按生产 2 秒获取超时配置，早期默认30秒池导致客户端截止时间先到的失败记录保留。独立成熟客户端实际刷新成功，新 ID Token 不回放授权 nonce，旧 refresh 重放使家族全部 inactive。

早期并发刷新时间戳在锁前采集可能违反撤销时间约束，已改锁内读取并保留失败记录；并发失败明确检查 invalid_grant，避免把限流或传输故障当作重放成功。

收尾补充真实验证：refresh的token-levelscope可缩为openid，userinfo及introspection不返回原email权限，再扩大被invalid_scope拒；refresh签名失败保留旧token未消费且新token总数不变。已在userrow锁等待中的真实logout对稍后创建的token使用锁后时间并全部撤销。preauthbound确认不能撤后续新登录session，另一浏览器sid也不能借原确认撤销。
