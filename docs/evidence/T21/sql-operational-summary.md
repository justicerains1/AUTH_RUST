# T21 管理与 Worker 主热点补充验收

2026-10-09 UTC。最后 [sql-operational-2026-10-09T00-29-44-890Z.json](sql-operational-2026-10-09T00-29-44-890Z.json) 于00:29:44.890～00:30:14.390实际退出0、testExitCode0，七个相关源码/锁文件摘要开始/结束一致。整个 [T21入口](../../../tests/integration/T21.mjs) 已实际运行通过原容量/测量逻辑单元、四项SQLObserver单元、原15计划/本人分页探测和本补充探测；本补充必跑，缺脚本/真实数据/查询计时不得返回通过。

## 实际权限与数据

[独立测试](../../../crates/identity-server/tests/t21_operational_queries.rs) 新建identity_test随机schema，100000合法用户与真实Argon2id摘要。管理员为其中既有用户，经`AdminStore::verify_bootstrap`真实校验密码、受控bootstrap事务、HTTP密码登录/近期密码、独立Node RFC TOTP enrollment proof取得资格；没有直接SQL设置strong_at或伪造认证。HTTP创建21客户端，测试真实注册outbox并用SMTP/Mailpit确认11封投递。测试只清自己schema，未停止PG/Redis/SMTP或运行长压测。

## 客户端 N+1 发现与修复

原版 [完整失败报告](sql-operational-2026-10-08T20-22-48-027Z-failure.json) 证明实际管理客户端limit1→20从9→47次SQL，多38=2×19。原代码分页取ID后每个client重复读详情/URI；[原观察](sql-operational-before.md) 和所有失败诊断保留，不被新通过覆盖。

根代理只在 [AdminStore::clients](../../../crates/identity-store/src/admin.rs) 最小修改：分页直接取完整client字段，只对实际返回ID集合发一条URI批量查询，按client/kind/uri归组，保留分页原顺序与cursor。原权限、近期因素、limit上限和客户端秘密规则不变。

| 实际管理端点 | limit1 SQL完成数 | limit20 SQL完成数 | 新报告 |
|---|---:|---:|---|
| 用户列表 | 7 | 7 | 一次真实HTTP各返回1/20行。 |
| 客户端列表 | 8 | 8 | 一次真实HTTP各返回1/20行，原9/47已消除。 |
| 审计列表 | 7 | 7 | 一次真实HTTP各返回1/20行。 |

计数来自SQLx实际完成事件，unknown_statement_count0、每次事件的elapsed都有数据，汇总timing count与完成数一致；不是源码静态计数。客户端返回字段在进程内逐项与API创建结果完全一致，包括scope、两条login/two logout回调顺序、名称/状态/时间；不把这些用户值写报告。两个limit页首ID相同；cursor下一页剩1、无重复且内容一致、最终无next_cursor；无效cursor拒绝。

另用同一实际管理员session的受权限仓储接口验证：合法signedcursor逐页返回21客户端、相同created_at fixture按id DESC不漏/重，disabled客户端仍列出，无logout回调保持空；精确删除本fixture客户端与回调后真实授权列表为0行、无cursor。直接仓储接口用于cursor边界，不能冒充HTTP预算检查；HTTP分页本身已单独验证并保留原限流，测试准备不超过每管理员30次/分钟预算。

## Worker SQL工作量与精确计划

实际注册产生outbox，Worker batch1与10均claim/delivered正确、无retry/failed，Mailpit看到邮件，数据库delivered且encrypted_params=NULL。SQL量5和32，按实际形状逐项验证：claim WITH UPDATE1、COMMIT(N+1)、delivery UPDATE N、audit INSERT N，即2+3N。每封独立持久化事务是明确工作量，不能误标为分页N+1；没有缓存、跳过审计或合并危险副作用。

八个直接来自实际仓储/Worker函数的typedbind EXPLAIN ANALYZE BUFFERS已执行，锁/写计划在独立事务rollback，不消费或残留租约：

| 真实SQL形状 | 本机单次Execution Time |
|---|---:|
| 管理员会话权限 / 当前因素 | 0.021 / 0.012ms |
| 用户分页 / 客户端分页 | 0.026 / 0.021ms |
| 客户端URI批量查询 | 0.052ms |
| 审计分页 | 0.016ms |
| outbox claim WITH UPDATE | 0.262ms |
| delivery UPDATE清密文 | 0.178ms |

这些是单次本地计划，不是负载p95或生产容量。已有原15计划覆盖session/introspection/password/refresh主要路径；本次覆盖三个管理主列表和Worker成功领取/完成。没有宣称所有后台变更、失败SMTP、任意用户数据分布或全部未来接口都无N+1。

## 脱敏与验证

[Observer](../../../tests/load/query-evidence.rs)小扩仅在test target：Worker静态SQLcatalog、精确函数体提取、写计划Triggers仅保留Time/Calls数字，删除名称/条件/输出/Sort/PresortedKey等表达式；未知plan/trigger字段仍失败。四项meaningful单元重新通过，包括原安全predicate、秘密条件删除及unknown拒、完成事件计数、缺elapsed/未知形状拒绝；原t21_load四项兼容验证也通过。Clippy -D warnings、Node语法/ESLint/diff检查通过，无新增依赖或放宽生产规则。

公开报告已核对零用户UUID/email、零Cookie/密码/client secret/token JSON字段，计划无IndexCond/Filter/Output/SortKey等绑定表达式。源码摘要在执行前记录、结束验证稳定，否则整体失败；所有早期计划schema/准备429/原N+1失败保留。

本补充使指定本地PERF03主热点有直接计划/调用数证据；是否放行由根按原验收决定。规定硬件、生产RUM、不同数据分布/长时容量与完整部署条件仍不能由本短实验推定；已通过的四个长负载场次没有重复运行。
