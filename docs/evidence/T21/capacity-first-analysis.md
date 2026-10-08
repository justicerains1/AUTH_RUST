# 首次短阶梯容量失败分析

2026-10-08 UTC，只读分析 [load-capacity-2026-10-08T15-48-43-290Z-failure.json](load-capacity-2026-10-08T15-48-43-290Z-failure.json)、[固定阶梯脚本](../../../tests/load/capacity.js)及其报告提交`7c7bc2f38b5ae21ae48fc324c8efc4ddb32bbe63`的生产调用路径。没有新测试、服务操作或源码修改。

这次是单一隔离fixture连续1/2/4/8倍负载、每档60秒预热+60秒测量的短实验，不是规定十五分钟验收。k6退出99、整体1；报告明确记录令牌池renew失败。**355 RPS短档有效，710 RPS档已出现非预期错误，不能据本报告宣布系统最大容量为355或710，也不能把后档无效身份都归为认证实现错误。**

## 分档实际结果

每档基准为300 introspection+50 account+5 password。表内请求数只计各档后60秒，RPS由实际count/60计算；k6 counter自带rate除整次480秒，不能当作该档实际速率。

| 倍率 / 目标综合RPS | introspection数 / 非预期错误 / p95 | account数 / 非预期错误 / p95 | password数 / 非预期错误 / p95 |
|---|---|---|---|
| 1 / 355 | 18000 / 0 / 5ms | 3000 / 0 / 1ms | 300 / 0 / 138ms |
| 2 / 710 | 36000 / 9057（25.158%）/ 11ms | 6000 / 0 / 2ms | 600 / 153（25.500%）/ 156.05ms |
| 4 / 1420 | 71944（目标72000）/ 49984（69.476%）/ 99ms | 12000 / 0 / 10ms | 1200 / 747（62.250%）/ 220ms |
| 8 / 2840 | 76669（目标144000）/ 63362（82.644%）/ 1003ms | 23939（目标24000）/ 0 / 282ms | 2394（目标2400）/ 2193（91.604%）/ 1433.35ms |

全程dropped iterations=130989，正常429计数0，invalid-authority计数14756。后两档实际请求数/延迟均有未达项；失败响应进入延迟统计，所以“p95小但大量错误”不是吞吐验收成功。此脚本的invalid-authority指任何非预期且HTTP<500/非429结果（例如active=false），不是越权成功标签。

## 错误时间线与 SQL 观察

API固定endpoint observer统计包含预热和四档，不能把其累计HTTP状态直接等同各档后60秒计数。每15秒资源采样使首次实际错误时间只能定位到相邻快照间，不能精确到请求。

| 首次可见快照 | 实际累计变化与资源 |
|---|---|
| 15:51:28.661 | 2倍预热期：introspection已503×424、csrf已503×7；hash queue timeout=0，refresh失败=0，pool size32/idle30。成功acquisition最大19.95ms。 |
| 15:51:43.678 | password出现503×2；hash queue timeout仍0、refresh失败仍0、pool idle31。 |
| 15:52:58.740 | token刷新出现503×94，renew failure=94；先前renew512次成功。此后令牌池有效性无法保证，脚本正确使整体失败。 |
| 15:55:14.106 | 8倍阶段首次采样见hash queue timeout=20、waiting高水位12，随后最终timeout168、高水位18。 |
| 15:56:14.079 | pool idle采样最低0；PG采样见WalSync/WALWrite等待，acquisition累计最大约0.675秒。 |

最终endpoint累计计数：

| endpoint | HTTP状态 | SQL完成观察 |
|---|---|---|
| account | 200×89736 | 89736次完整me查询，始终每请求1SQL。 |
| introspection | 200×190973，503×218325 | Basic客户端查询190973次、完整token权威查询190973次，合计2×200；SQL min/max=0/2。 |
| csrf | 200×3830，503×5159 | INSERT+COMMIT共7660次=2×200；SQL min/max=0/2。 |
| token（后台renew） | 200×967，503×473 | SQL完成12571次=13×200；SQL min/max=0/13。 |
| password | 200×2820，503×1010 | SQL min/max=1/22；后期包括已读取用户但哈希排队拒绝。 |

四种失败路径的SQL聚合与0-query下界显示，introspection/csrf/token的失败落在数据库业务查询之前；它们成功请求仍执行预期SQL数量，unknown_statement_count均0。此证据不直接记录逐个503的内部错误分支，不能证明每个503恰由同一个错误产生。

introspection完整查询190973次只返回154370行：36603次HTTP200实际为inactive，与invalid-authority/renew失败后的token过期相符，但没有逐token追踪证实一一因果。renew473次失败后，部分家族未及时换新access；后档和后续无效身份不是独立新fixture实验，不能作为纯容量上限。短档升压与120秒批量renew时间邻近，但未采集因果相关耗时，不能断言批量刷新导致初次错误。

## 资源说明

34份快照，API RSS24.33～245.94MiB、进程HWM最高355.42MiB；k6有效31份RSS1654.32～4725.60MiB。PG Docker CPU4.25～58.39%、内存232.9～355.2MiB；Redis CPU0.24～25.62%、内存8.312～24.86MiB。API/PG/Redis/k6同机、无额外资源上限，不能把负载器压力、OS调度/文件描述符/网络或服务压力分开归因。

池size2～32/idle0～31；观察到的PG active0～3，多数连接为idle/ClientRead，个别后期idle-in-transaction及WalSync/WALWrite。SQLx成功获取连接的最终mean/max：account33.81/674.63ms、introspection33.07/675.46ms、password43.31/676.26ms、token11.34/544.51ms。这些成功acquisition数据包含整个实验；没有失败获取计数/原因，不能仅凭size32判断初期池饱和，也不能凭离散idle高证明全程无等待。

最终Argon2参数仍64MiB/t3/p1，slot/running高水位各4；验证2821次平均计算140.64ms，queue timeout168，等待高水位18。排队超时是8倍后期明确限制，但710档错误已先出现且当时queue timeout0，所以不足以解释首次错误。没有降低安全参数的证据或理由。

## Redis 连接路径的可疑方向

报告提交的 [SecurityState::check_limit](../../../crates/identity-server/src/security.rs) 每次请求执行 `get_multiplexed_async_connection()`，随后同一Lua原子更新预算；建连与Lua共同受2秒timeout，失败统一返回BoundaryUnavailable。introspection/refresh先调用此边界再查PG，csrf也在创建preauth前调用，password在读取用户前调用。account GET不执行这项Redis预算，710档account仍有效，因此存在与实际错误分布一致的检查方向。

锁定redis1.7.1实现该方法每次调用`get_simple_async_connection`创建新连接并spawn独立driver；“multiplexed”描述所得连接可以并发共享，不表示`Client`自动复用上次连接。现有代码未缓存连接对象。报告没有Redis连接attempt/failure/timeout或Lua invocation耗时，不能据源码的建连频率直接宣布连接耗尽/Redis瓶颈已证实。

## 下一次需要的真实定位数据

- 将Redis建连与Lua调用分别记录固定类别attempt/success/error/timeout和耗时bucket，另外记录总边界耗时；不含IP/account/Redis key、Cookie、secret或原Redis错误正文。对503记录固定失败阶段，保留2秒失败关闭和原Lua预算。
- Redis采集connected_clients/total_connections_received/rejected_connections、命令调用/延迟与有限错误类别；同时采集API/k6进程CPU/FD数量、主机调度/网络连接压力。没有新记录前不要把CPU快照代替连接压力证据。
- 按档区分实际HTTP status、active=false、传输错误和token-pool renew status/原因/最后成功时间；升压后renew失败应终止或另开独立fixture测试，不能继续污染后档再称有效容量。
- 保持原355 RPS十五分钟基准和安全参数，分别验证恢复/共享连接行为后再做有定位指标的短阶梯；比较相同数据/机器/速率，不能以降低阈值或关闭限流使结果变绿。

本文件只确定了失败范围和下一步定位方向，没有修改生产逻辑或断言根因已解决。正在进行的独立完整mixed实验应等待自己的原报告，不由此短阶梯推定成功或失败。
