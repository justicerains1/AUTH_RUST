# T21 资源、热 SQL 与容量证据边界

2026-10-08 UTC。只读分析 [plan.md](../../../plan.md) 的 T21.05/.06/.09 与四份已完成负载原报告；没有新增压测、查询、故障注入或服务操作。本文件补充 [load-summary.md](load-summary.md)，不修改验收状态。

当前证据证明：在这台 WSL2/Ryzen 7 7700X、16 logical CPU/15.217 GiB、API/PG/Redis/k6 同机环境，指定混合 **300 状态+50账号+5密码=355 RPS** 持续900秒，目标请求数全部完成且零 dropped/非预期错误/正常429/无效身份。355 RPS 是这个工作负载已测得的可承载下界，不是最大容量、参考环境结论或允许提高生产流量的承诺。没有进行更高速率/更高密码占比/更多应用/资源限额/长时数据增长的饱和测量。

## 原始来源与计算口径

| 场景 | 已测目标 | 原报告 |
|---|---|---|
| introspection | 300 RPS | [load-introspection-2026-10-08T10-29-20-020Z.json](load-introspection-2026-10-08T10-29-20-020Z.json) |
| account | 100 RPS | [load-account-2026-10-08T11-05-59-379Z.json](load-account-2026-10-08T11-05-59-379Z.json) |
| password | 5 RPS，CSRF+POST完整流程 | [load-password-2026-10-08T11-24-05-797Z.json](load-password-2026-10-08T11-24-05-797Z.json) |
| mixed | 300+50+5 RPS | [load-mixed-2026-10-08T11-44-37-116Z.json](load-mixed-2026-10-08T11-44-37-116Z.json) |

每场120秒预热+900秒测量，原报告各70份资源快照，`unavailable` 为0。快照约15秒采样，包含预热前后/结束采样；下方资源范围不是仅测量期统计，也不代表连续采样捕获的瞬时峰值。API `VmRSS`、`VmHWM` 的 Linux kB 数值除1024转MiB；Docker `MemUsage` 原值为MiB。k6每场67份有RSS，另3份为空（启动前/结束后未读到），不把缺值当0或声称完整连续监测。

API CPU由 `/proc` 的 user+system 累计 ticks计算；本机只读 `SC_CLK_TCK=100`。平均使用完整首末快照约1022.8秒，区间范围只取相邻10～20秒采样，排除结束重复/极短间隔。100%代表占用一个逻辑CPU，不是16核主机总百分比，也不与Docker统计的单独时间窗口相加。Docker CPU列直接引用70个`docker stats --no-stream`快照范围。

## T21.06 已观察的资源范围

| 场景 | API RSS MiB / 最后进程HWM MiB | API CPU单核等价平均 / 约15秒区间 | PG CPU / 容器内存 MiB | Redis CPU / 容器内存 MiB | k6 RSS MiB |
|---|---|---|---|---|---|
| introspection | 22.90～28.97 / 75.73 | 16.05% / 12.28～23.25% | 0.14～34.23% / 209.3～268.7 | 0.23～10.53% / 20.17～22.70 | 486.24～696.50 |
| account | 22.76～25.15 / 75.65 | 3.13% / 1.93～9.33% | 0.09～27.07% / 205.6～215.6 | 0.19～5.48% / 19.97～25.73 | 434.86～595.84 |
| password | 22.66～87.00 / 87.00 | 52.88% / 48.67～66.32% | 0.06～23.49% / 207.3～222.3 | 0.26～4.90% / 23.82～28.61 | 104.20～120.18 |
| mixed | 22.34～195.90 / 290.71 | 77.40% / 63.45～117.36% | 0.05～44.14% / 211.6～341.1 | 0.28～14.43% / 22.52～26.41 | 869.35～1309.09 |

`VmHWM`是该测试进程生命周期最高RSS，包含seed/setup，不可当作900秒测量期精确峰值。综合RSS采样最大195.90MiB、进程HWM290.71MiB，二者差异不能定位峰值属于setup还是测量期。Docker容器内存是工具的cgroup内存统计，不是进程RSS。PG/Redis容器没有额外CPU/内存上限，负载器与数据服务共用主机；以上不能推断8vCPU参考机器余量。

| 场景 | SQLx实际size / idle / size-idle | PG active connections | 密码验证数 / 累计计算耗时 / 均值 | queue timeout / refresh失败 |
|---|---|---|---|---|
| introspection | 2～18 / 1～17 / 1～4 | 1～2 | 0 / 0 / 无验证均值 | 0 / 0 |
| account | 2～3 / 0～2 / 1～3 | 1～2 | 0 / 0 / 无验证均值 | 0 / 0 |
| password | 2～3 / 0～2 / 1～3 | 1～2 | 5101 / 518.492秒 / 101.645ms | 0 / 0 |
| mixed | 2～32 / 1～31 / 1～12 | 1～3 | 5101 / 599.992秒 / 117.622ms | 0 / 0 |

SQLx上限在 [测试 harness](../../../crates/identity-server/tests/t21_load.rs) 固定32；size是已创建连接数，idle是可用连接数，二者快照差仅为近似占用量。综合size达到32不能等同池耗尽，采样idle最低1也不能证明全过程无等待。`pg_stat_activity`查询按整个测试数据库`state='active'`统计，包含自身采集查询/其他连接；它不是专属API活动数。原采集查询出错会fallback 0，因此低数值本身不构成数据库无压力证明；这四报告观察值为1～3且外层快照无 unavailable。

Argon2id保持64MiB/t=3/p=1、并行上限4；每场seed的一次真实hash为94.57～132.47ms，不能当作密码验证分布。密码5101次计数包含预热，累计耗时/次数只给平均计算成本，不包括完整HTTP/CSRF和队列等待，不能替代p95。每场真实refresh4096次且失败0。

尚未采集：哈希队列深度/实际并发高水位/等待时间直方图，SQLx acquisition wait/timeout分布，PG锁等待/等待事件/长期连接峰值，连续主机和SSD延迟/内存压力。queue_timeout=0只表示这次未达到队列拒绝截止，不证明完全没有排队。当前没有证据需要降低Argon2参数或提高安全并行上限。

## T21.05 热 SQL 的实际范围

真实种子100000用户/100000授权/20客户端，账号流量读取2048个合法会话池；这是在十万行数据库上执行真实接口，并非逐行遍历十万用户。原报告每场只有两份setup时单个选定用户/grant的 `EXPLAIN (ANALYZE,BUFFERS,FORMAT JSON)`：

| 已执行等价查询 | 四场 Execution Time | 实际计划 |
|---|---|---|
| `account_equivalent_index_path` | 0.022～0.055ms | sessions_active_user_idx Index Only Scan + users_pkey Index Scan，返回1行。 |
| `introspection_equivalent_join_path` | 0.038～0.060ms | grant/user/session主键或活动索引联合，oauth_tokens_grant_idx Bitmap Index/Heap Scan，返回2个token行；20行clients表Seq Scan。 |

这些是单次本地setup计划，没有负载期SQL耗时分布；小clients表Seq Scan本身不足以认定缺陷。它们按user/grant定位以省略秘密token摘要，**不等同真实保护请求按session/access摘要查找的全部SQL**。等价查询没有完整time/version/kind/token/client归属条件，返回两个token行也不证明单access权威查询成本。不能据此宣布全部热SQL、无N+1或锁竞争达标。

仍待完整热路径审查：本人session权威检查、真实introspection token/client连接、密码用户读取与user锁后版本复核/会话插入、refresh轮换/签名提交、设备/授权/管理员cursor分页、审计/outbox写入和Worker领取。需要从实际SQL形状保存脱敏计划/调用数、负载期耗时及buffer/锁等待；秘密摘要只用等长合成绑定值或受控本地参数，不写真实凭据到报告。已有 [T19 十万用户分页](../T19/test-boundaries.md) 的单次索引计划可补充，但不能代替上述完整路径。

## T21.09 已知容量边界与后续判定

本次支持的结论仅为指定工作负载与数据规模下的有限持续能力。最高已执行混合流量355 RPS，其中密码5 RPS；它不是不同密码占比、更多客户端、TLS/公网网络、真实邮件、备份任务、不同机器或长时数据增长下的保证。尚无饱和拐点、最大RPS、连接池/哈希并发峰值或生产SLO，不能给未测的数值上限。

扩展/限流应先看实际指标：若数据库锁/IO/池等待主导延迟，先处理对应热SQL、索引和数据服务资源，再评估数据库拆分；若API自身CPU/哈希任务主导而数据层有余量，评估增加API副本及共享锁/密钥一致性，仍需T24独立验收；若出现持续非预期错误、队列超时或达不到文档延迟门槛，应控制进入流量、按runbook恢复和重新测量。这里只说明决策依据，没有设定未经实测的扩容阈值，也不提高已配置的安全预算。

规定Linux8vCPU/16GiB/SSD环境、完整热SQL/等待指标、容量拐点及生产RUM/真实高峰仍未完成。可以依据现有报告完成本说明，但T21整体和生产发布继续按原验收关卡判断；不为补文档追加压测或删除失败结果。


后续本地补全：精确15份SQL计划及实际每请求次数/分页1-20项稳定性已执行，见[SQL补充](sql-summary.md)；新增等待/并发高水位真实综合900秒见[观察场次](observed-mixed-summary.md)。共享Redis连接及令牌快照修复后的短阶梯见[当前容量](capacity-current-summary.md)。这些新增证据更新本文件所述历史“尚未采集”中的对应项，未覆盖的全部业务热SQL、连续主机/SSD统计、规定硬件和生产样本仍需验收。
