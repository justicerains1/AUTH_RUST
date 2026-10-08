# Redis 共享连接与失败关闭验证

2026-10-08 UTC。生产变更仅 [SecurityState](../../../crates/identity-server/src/security.rs) 的限流传输复用；原Lua预算、HMAC键、Origin/CSRF、数据库权威状态与认证策略不变。相关真实测试为 [t04_security.rs](../../../crates/identity-server/tests/t04_security.rs)，新案例已加入原 [T04 runner](../../../tests/integration/T04.mjs)，没有替换或删除既有故障/并发案例。

## 最终行为

- 同一SecurityState共享MultiplexedConnection的clone，并发Lua不持连接槽或重连锁。首次没有连接时使用single-flight重连gate，获得gate后二次检查，实际只建立一个socket。
- 缓存槽含单调代次；命令失败/超时/取消仅清理相同代次，旧请求较晚失败不会误删已经发布的新连接。后续新请求才能重新连接。
- 每次限流检查总截止仍两秒，包含gate等待、建连与Lua；redis1.7.1原默认响应截止500ms保持，内层Redis超时计入timeout。没有降低安全截止或提高预算。
- 命令失败/超时/取消可能已经计数预算，本请求直接失败关闭，不自动重试Lua。redis-rs原NOSCRIPT处理会先加载脚本再EVALSHA，首次NOSCRIPT未执行Lua，不能与失败后的重放混淆。
- 每请求只缓存传输连接，不缓存active/身份/CSRF/权限/限流结果。Redis不可用时相关入口仍503，不旁路认证。
- 新指标仅gate/建连/调用attempt-success-failure-timeout-cancel与耗时bucket、reuse/invalidations/generation/cached；无用户/IP/client/Redis key/凭据标签。原子字段逐个采样，不保证跨字段绝对同瞬间一致。

## 实际验证

最终 [integration.txt](integration.txt) 记录16:57:44.768～16:58:08.685 UTC，整个T04套件退出0，包括原CSRF/代理/真实Redis十并发预算、邮件预算/HTTP429、AEAD、preauth截止、真实Redis和PG故障。

新增同一服务实例的真实Redis案例：建立观察用连接后读取 `INFO stats` 的total_connections_received基线，20并发Status预算检查后增量精确1；服务指标physical connect attempts/success各1、reuse19。实际停止Redis，原已缓存连接的CSRF入口返回503，命令未自动重试，缓存代次失效；恢复并等待healthy后先建立新观察连接再读基线，原SecurityState的20并发检查仅新增一个替代socket、代次增加1，最后CSRF数据库持久化200。

观察连接在每次基线前建立并全程复用，故其本身不混入增量；检查使用Status预算和不同测试来源，未因Preauth30预算触发假限流。数据库为独立identity_test随机schema、真实迁移，结束只清自己schema。

九个security lib单元实际0failed/0ignored（其中五个新增连接行为）：旧代次不能失效新槽、20并发真实TCP握手只一连接、已发送命令超时仅一EVALSHA不重试、取消命令失效后后续能重连、gate等待计入总deadline。合成RESP服务用于明确响应延迟/取消窗口，不冒充真实RedisLua业务；原并发与停机证据来自T04真实依赖。

最终identity-server all-target Clippy `-D warnings`、T04脚本ESLint和diff检查均0。[redis-connection-final-health.json](redis-connection-final-health.json) 实读确认Redis、PostgreSQL、Mailpit都running/healthy，故障窗口已释放。

## 失败保留与容量限制

最初T04恢复案例连接恢复断言已成功，但随后CSRF200断言失败：测试新state使用了未迁移默认测试库，无持久化表；已补专属schema/迁移，未修改生产成功规则或把503改成期望成功。保留 [首失败](redis-connection-recovery-first-failure.txt)、[安全诊断](redis-connection-diagnostic-1791478508512.txt)。开发中类型/feature/Clippy错误均修到实际通过，没有添加Tokio feature或关闭lint。

首次容量阶梯的503在PG查询前、每次新Redis连接是对应调查方向，见 [capacity-first-analysis.md](../T21/capacity-first-analysis.md)。本变更及故障验证证明连接确实复用/恢复，**尚不能声称容量根因已解决或710RPS可承载**；根需以同数据/同阶梯及完整mixed长测的新二进制报告复核，原容量失败永久保留。


后续相同短阶梯实际复测表明355/710/1420RPS三个60秒测量档无错误、真实Redis1连接/全部调用0失败，2840档密码四任务排队超时及负载器drop明确限制；参见[当前容量结论](../T21/capacity-current-summary.md)。仍非参考硬件或900秒结果，原“尚不能”段保留修复当时边界，最终综合长测另独立记录。
