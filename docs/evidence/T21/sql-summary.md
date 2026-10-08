# T21.05 精确 SQL 与真实接口调用证据

2026-10-08 UTC。最新短时真实取证 [sql-probe-2026-10-08T15-06-29-301Z.json](sql-probe-2026-10-08T15-06-29-301Z.json) 退出0：100000用户、20客户端、100000活动授权、512个真实授权码兑换的token家族、2048个合法会话池。此次只执行setup/短探测，预热和长测秒数均0，没有运行k6，不替代十五分钟负载或规定硬件验收。

## 精确计划来源与安全边界

原证据中的两个等价简化查询已替换为15个直接从 [实际仓储源码](../../../crates/identity-store/src/tokens.rs) 与sessions/security/repository函数抽取的SELECT字面SQL。计划保留真实session/token摘要查找、客户端secret/version/time/consumed条件、scope/归属联合及事务锁；`ANALYZE`后，使用本次隔离fixture的真实typed binds执行 `EXPLAIN (ANALYZE,BUFFERS,FORMAT JSON)`。锁查询在单独事务中执行后rollback，不消费凭证或改变认证状态。

| 仓储函数/路径 | 实际单次计划耗时 | 当前证明 |
|---|---:|---|
| sessions.me | 0.060ms | 完整本人身份/安全信息查询返回1行，子查询因素/Passkey/恢复码/管理员状态仍在同SQL。 |
| security.valid_session / repository.session_authority | 0.015 / 0.018ms | 当前session/user/credential version与时间条件。 |
| sessions.credential_by_email / complete_password_login用户锁 | 0.025 / 0.016ms | 精确用户读取与密码校验后user行锁复核。 |
| tokens.authenticate_client / introspect | 0.009 / 0.054ms | Basic客户端读取和完整token权威联合。20行client表Seq Scan不自动认定缺陷。 |
| sessions.session_page / tokens.grant_page | 0.026 / 0.030ms | 原始cursor/排序/limit SQL，setup所选账号返回1行。 |
| refresh定位/user锁/session锁/grant锁/family锁/旧token检查 | 0.020 / 0.011 / 0.015 / 0.013 / 0.010 / 0.013ms | 完整只读定位与真实锁形状；family锁返回2行。 |

以上是本地setup时单个绑定对象的执行计划，不能当作负载p95、所有账号数据分布或容量上限。计划JSON使用明确字段正schema，删除`Index Cond`、Filter/Recheck/Join条件、Output、Sort/Group/Cache Key及Subplan表达式；不写用户UUID/email/token摘要/Cookie/密码/客户端secret。未知字段或缺/不合法计时导致失败，不忽略风险字段。源码SQL本身只有参数位置和固定业务常量，没有实际绑定值。

## 真实 HTTP 的 SQL 数量

[测试Observer](../../../tests/load/query-evidence.rs)只存在于test target。SQLx query完成事件提供实际参数化形状、耗时、返回/影响行数；固定endpoint span归集，SQLx pool实际acquisition事件另计。没有fmt/raw SQL输出层，没有捕获或打印bind。missing elapsed、未知形状或请求完成超时使证据失败。一次HTTP可能执行同形状多次，因此下表按query完成次数计数，不是不同SQL形状数量。

| 实际端点 | 真实请求数 | 每请求SQL完成数 | 观察结果 |
|---|---:|---:|---|
| GET /api/v1/me | 3 | 1 | 一次完整身份查询，无额外每因素查询。 |
| GET /api/v1/me/sessions，limit=1 | 3 | 3 | 包含身份查询与分页执行。 |
| GET /api/v1/me/sessions，limit=20 | 3 | 3 | 实际返回20条，完成数与1条相同。 |
| GET /api/v1/me/grants，limit=1 | 3 | 3 | 身份/权威与分页查询。 |
| GET /api/v1/me/grants，limit=20 | 3 | 3 | 实际返回20条，完成数与1条相同。 |
| POST /oauth/introspect | 3个独立真实token，各1次 | 2 | Basic客户端读取+完整token权威查询各一次，active=true。 |
| POST /oauth/token，refresh | 1 | 13 | 当前合法refresh真实轮换成功；包含锁、旧token检查、两行insert、审计及commit，新pair写回池。 |
| POST /api/v1/auth/login/password | 1 | 22 | 正确密码产生authenticated，包含CSRF当前检查、锁后版本、会话/事务/审计；前置GET csrf另外2次SQL。 |

所有probe的unknown_statement_count为0，SQL完成timing count与请求归集一致。limit1/20比较前只在本次随机schema明确增加25会话/授权行，完成后按生成ID精确删除，恢复100000活动授权；不改变seed认证事实或生产数据。该对比证明**受测本人session/grant分页没有随返回行数增长的仓储调用N+1**，不宣称所有管理员/Worker/业务路由都无N+1。密码和refresh各一次probe也不构成并发饱和或统计分位数。

## 等待/排队采集与复用

测试PgPool显式启用query完成和acquisition时长事件，上限32、获取截止2秒保持。15秒资源采样可累计每endpoint的请求/SQL耗时bucket、行数、SQL-count min/max与acquisition count/耗时bucket，供根下一次完整负载执行。SQLx日志未发送到stdout；固定源码catalog只公开被实际执行的参数化形状。

PG等待采样只过滤本次随机 `application_name`，排除采集连接，汇总state/wait_event_type/wait_event及数量，不暴露pid/query/用户/客户端标识；查询失败返回503而非假零。最新短测的一次快照为本测试池1个idle/ClientRead、active0；这是实际只读快照，不是长期无锁等待证明。

根新增PasswordService指标已接入：waiting/slot/running当前与高水位、等待累计nanoseconds与8桶。短测只有1次真实密码验证，waiting/slot/running高水位均1、queue timeout0，累计等待2700ns；不将该样本作为长测峰值。原子字段逐个读取，snapshot不保证彼此在同一瞬间一致。新字段的并发/取消/permit生命周期单元由根验证；本报告仅证明采集接口实际可读。

## 修复、复核与保留的失败

四个meaningful单元已实际退出0/0ignored：精确源码安全predicate不丢失、条件值删除且未知plan字段拒绝、完成query/acquisition事件按请求计数、缺elapsed/未知SQL明确invalid。它们使用合成事件检验采集逻辑，不冒充数据库或认证成功。真实HTTP/PG成功证据来自上述独立短探测。

首轮Subplan标签/后续新计划字段触严格schema失败，按公开结构字段补处理、表达式整体删除；fixture会话ID误作userID触真实FK拒绝，修正绑定并保持约束；早期错Cookie名引发真实401，改为既有`identity-dev`；分页额外fixture未清理造成100025授权数被严格runner拒绝，修后定向清理。所有失败JSON和脱敏harness记录保留，没改安全策略/阈值或把失败覆写成成功。早期SQL错误用户UUID已脱敏，后续错误仅输出固定原因和数据库code。

[sql-source-verification.json](sql-source-verification.json)记录最新原报告SHA-256、九个源码/锁文件摘要与当前文件逐个一致，另复核公开计划无表达式值/UUID/email/凭证JSON字段。没有追加同样短测：最后成功报告覆盖refresh与wait_event字段，源码一致复核及四单元再次通过即完成本子模块。

当前完成的是精确受测路径和可重放采集。本机短测不匹配Linux8vCPU/16GiB/SSD参考；完整十五分钟新指标、更多路由SQL/N+1、等待分布/容量拐点和生产RUM仍由对应实测决定，不能从该报告直接放行T21或生产。
