# 管理分页与 Worker 热点的原始观察

2026-10-08 UTC。[sql-operational-2026-10-08T20-22-48-027Z-failure.json](sql-operational-2026-10-08T20-22-48-027Z-failure.json) 为完整真实操作取证，退出1，实际拒绝原因是客户端分页SQL数随返回行数增长。它不是预期通过或生产发布结果，不以修复后报告覆盖。

测试隔离schema内创建100000个合法用户、真实Argon2密码摘要；首管理员通过`verify_bootstrap`核验既有账号实际密码、bootstrap受控事务、HTTP密码登录/近期密码及独立RFC TOTP enrollment确认取得权限。没有SQL修改strong_at或模拟认证成功。客户端由真实管理员API创建，列表和审计都发真实请求；SMTP保持运行，没有故障/长压测。

| 实际列表 | limit=1 SQL完成数 | limit=20 SQL完成数 | 结果 |
|---|---:|---:|---|
| 管理员用户 | 7 | 7 | 实际1/20条，调用数固定。 |
| 管理员客户端 | 9 | 47 | 增加38=2×19，原查询逐行读取client与redirect列表。 |
| 管理员审计 | 7 | 7 | 实际1/20条，调用数固定。 |

SQLxObserver只按实际完成事件计数，参数只留内存，unknown_statement_count=0。客户端N+1来自`AdminStore::clients`先分页取ID，再每条调用`client_view`的两条查询；根代理将单独最小批量修复，取证代理不更改生产文件。

Worker随后实际HTTP注册创建outbox，batch1和batch10均通过真实SMTP投递、Mailpit读取和数据库payload已清验证。SQL数分别5和32：单次claim WITH UPDATE+COMMIT共2，每封delivery UPDATE+audit INSERT+COMMIT共3，公式2+3N。这是每封独立持久化事务的明确工作量，不能作为分页N+1缺陷。

七份exact EXPLAIN包含管理员权限/因素/用户/客户端/审计分页、真实claim UPDATE以及完成UPDATE；计划数据只保存白名单结构与数字，条件值、输出、UUID/email/凭据都不在报告。早期schema处理缺IncrementalSort/trigger字段或测试准备触限流的失败均保留；没有改预算、时限或关闭约束来填通过。

本证据只覆盖列出的管理主列表与Worker成功领取/完成，不宣称所有后台/Worker异常或生产容量。批量修复后的同口径完整复测另存新报告，并需核对真实scope/回调顺序/分页结果保持。
