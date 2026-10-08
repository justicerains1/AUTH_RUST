# T21 后端负载子模块方法与工具

本模块实现隔离种子、真实 HTTP 负载和资源记录，不自动标记 T21 整体验收通过。正式十五分钟测量应等待其他测试停止 PostgreSQL/Redis 故障注入；共享依赖受干扰的中断记录保留为不完整实验。

## 工具核查

k6 精确版本 2.3.0，官方 <https://github.com/grafana/k6/releases/tag/v2.3.0>，2026-09-21 发布。已读取该 tag 的 LICENSE.md（GNU Affero GPL v3）。它作为独立本地压测工具运行，不加入 Rust/npm 应用运行依赖。二进制来自官方 Linux amd64 archive，实际 `k6 version` 为 `k6 v2.3.0 (commit/e088784614, go1.26.8, linux/amd64)`。

官方 archive SHA-256：`39c3117b6af817592dcd0ce4242105c0a7af10948c2a425306f0be8f7a8a8ab1`；GitHub asset digest 与官方 checksums 文件一致，下载后已实际核对。每次负载运行仍验证 archive hash，版本必须 2.3.0。工具安装在未提交的 `.local/t21-tools/`。

## 命令与隔离

```sh
node tests/load/run.mjs --seed-only
node tests/load/run.mjs --scenario=introspection
node tests/load/run.mjs --scenario=account
node tests/load/run.mjs --scenario=password
node tests/load/run.mjs --scenario=mixed
```

只接受 `APP_ENV=test` 与 `identity_test`，数据库必须明确 loopback PostgreSQL URL，拒绝 query 目标覆盖参数和 production。未提供 URL 时从本地 development 配置读取连接信息，再明确选测试数据库，不写 development schema。Rust `MigrationTarget` 再核查实际目标，创建 `identity_test_t21_<随机 UUID>` schema，只迁移和清理自己创建的 schema。production、非白名单数据库和 query 覆盖已实际拒绝退出 1。

每次运行生成随机 0700 私有目录和 0600 密钥/凭据文件。k6 读取本地文件，令牌和密码不作为 CLI 参数、输出日志或性能报告。服务端测试 harness 只在 `cargo test --test t21_load` 存在；API 固定 loopback 5310，控制服务 loopback 5311，随机控制密钥保护令牌池与监控。未显式 `T21_LOAD_HARNESS=1` 时拒绝。停止后销毁凭据文件、schema 和测试服务。

## 数据、安全参数与实际身份

种子为 100,000 verified 用户、20 个机密客户端、100,000 活动授权和相应身份会话；服务端 CSPRNG 生成 UUID、会话/CSRF/客户端秘密。大量用户使用同一真实 Argon2id PHC fixture，密码由真实 `PasswordService` 哈希，64 MiB、t=3、p=1；并行上限 4，队列安全参数保持不变。批量 seed 是受限测试 setup，不冒充开放注册业务流程。

512 个访问/刷新家族由真实授权码 HTTP 交换创建，均摊到 20 个客户端，每客户端初始化最多 26 次，低于真实 Basic client 限流。每 120 秒对每个家族真实 refresh，保存新的访问/刷新值；不重用已消费 refresh，access 仍受五分钟到期。k6 setup() 一次预载当前 access 池，各 VU 首次使用该快照，之后以控制端点读取当前 access 池，每 VU 至多每分钟一次；控制端点耗时不算 introspection endpoint 指标。控制刷新失败单独记录并使结果失败，不能把令牌过期/伪造 active 当有效负载。

账号查询使用 2,048 个服务端合法随机身份会话池。密码登录逐次读取真实 CSRF，再带 Cookie/Origin/CSRF 调用真实密码登录，确认 authenticated 响应。测试代理仅信任 loopback 来路，以测试网段分散正常用户来源；账号池/IP 池避免容量测试被每账号五次/分钟和 IP 限流扭曲，安全限流始终启用，429 另计且正常场景必须为 0。

## 时长、指标与真实环境

每个独立场景使用连续 17 分钟 constant-arrival rate：前 120 秒预热，再 900 秒测量，复用同一批预分配 VU；无缩短选项。每个请求在发起时记录其 scenario 时间，只有 [120 秒, 1020 秒) 内开始的请求进入测量，结束时间越过边界也不丢弃。单项分别 introspection 300 RPS、普通账号 `/me` 100 RPS、密码登录 5 RPS；综合同时 300 状态 + 50 普通 + 5 密码 RPS。仅测量阶段进入自定义 endpoint Trend/错误计数；Basic HTTP 客户端认证包含在状态请求总耗时，密码 CSRF+POST 包含在完整密码流程耗时。

门槛保持计划值：introspection p95≤100 ms/p99≤250 ms，账号 p95≤200 ms，密码 p95≤1000 ms，非预期失败 rate<0.001；无无效身份、正常429或 dropped iterations。每 endpoint 还要求达到 900 秒目标请求数，防止只报告低实际 RPS 的漂亮延迟。warmup k6 内建总体数据单独可见，不混入自定义测量阶段 Trend。

每十五秒记录真实 API 子进程 CPU ticks/RSS/线程、PG/Redis docker CPU/memory/IO、SQLx 池容量/空闲与 PG active connections、Argon2哈希/验证总耗时和 queue timeout。查询计划采用非秘密参数化等价索引/联合查询，实际端点另行计时；后续 T21-PERF-03 仍需针对完整热 SQL 和容量边界进一步分析，不以等价计划宣称所有查询优化完成。

实际主机 WSL2 Linux、Ryzen 7 7700X 16 logical CPU、15.217 GiB；API、PG、Redis及 k6 同机，无额外 CPU/memory 限额。与参考 Linux 8 vCPU/16 GiB/SSD 不同，不能凭本子实验称参考环境后端性能达标或给出无限容量保证。

种子初步实测已创建全部数量、完成 512 个真实 HTTP code exchanges，结束后清理自己 schema；原始 seed-only 报告保留。首个 introspection 实验因共享 T20 故障测试协调而停止，k6 exit105，未完成十五分钟；原始输出和协调说明保留，不作为完整性能通过。正式结果以随后完整运行的 timestamp JSON 为准。

首个完整十五分钟状态场次 [load-introspection-2026-10-08T09-24-01-720Z-failure.json](load-introspection-2026-10-08T09-24-01-720Z-failure.json) 记录 p95=4 ms、p99=5 ms、业务错误/429/无效身份为0，真实刷新无失败，但只完成269,441/270,000请求并有560个 dropped iterations，严格判失败。后续修正压测端预载、连续预热/测量复用VU，并将目标计数保持严格rate×900，不降低阈值。期间两个不完整调试运行和一次并发lock变化导致的启动失败也保留原始报告；不将它们当作成功测量。

连续场次 [load-introspection-2026-10-08T10-07-42-441Z-failure.json](load-introspection-2026-10-08T10-07-42-441Z-failure.json) 完成269,985请求，仍有15个 dropped iterations，业务错误/429/无效身份为0，p95=4 ms/p99=5 ms。压测 iteration 最大约2018 ms，真实业务 Trend 最大91 ms，测试控制池刷新时持有整池锁导致辅助读取等待。修正仅针对测试 harness：HTTP refresh 期间不持有整池写锁，保存单个令牌对时短暂更新；仍串行消费每个真实 refresh，未改生产认证或限流。随后重新运行完整场次，不将此失败抹除。

修正测试池锁后完整状态场次 [load-introspection-2026-10-08T10-29-20-020Z.json](load-introspection-2026-10-08T10-29-20-020Z.json) 精确完成 270,000 请求（300 RPS × 900 秒），dropped iterations=0、业务错误/429/无效身份=0，p95=4 ms、p99=5 ms、最大18 ms；4096次真实refresh全部成功。该结果只对应当前非参考硬件的状态检查子场景；账号、密码及综合场景尚未执行，不能据此标记T21整体通过。

普通账号场次 [load-account-2026-10-08T11-05-59-379Z.json](load-account-2026-10-08T11-05-59-379Z.json) 完整900秒精确90,000请求（100 RPS），dropped=0、业务错误/429/无效身份=0，p95=1 ms、最大10 ms。密码场次 [load-password-2026-10-08T11-24-05-797Z.json](load-password-2026-10-08T11-24-05-797Z.json) 完整900秒精确4,500请求（5 RPS），dropped=0、业务错误/429/无效身份=0；含CSRF获取的完整登录流程p95=132 ms，密码POST端点p95≈127.8 ms，Argon2队列超时为0，安全参数未改变。综合场次此时仍在进行，以后续完整报告为准。
