# T23 运维发布审计

2026-10-08 UTC。依据[plan.md](../../../plan.md)第4.3节、T22.01～T22.11、T23.04～T23.09及[acceptance.md](../../../acceptance.md)的T22-OPS-01～04核对实际源文件和证据。原只读审计发现真实缺陷后获得明确修复授权；[修复及真实测试](ops-repair-summary.md)保留成功/失败记录。本报告不替代全量测试、最终镜像验证或正式部署。

T22/T23仍不能放行：已有本地制品/加密PITR/AEAD维护/指标证据和本次12项独立运维修复测试，但生产主机/域名、真实SMTP、独立存储、每日保留、完整账号恢复、签名分发和可达告警尚未验收。更完整的设备/浏览器/性能清单见[remaining-verification.md](remaining-verification.md)。

## 配置与制品

生产Compose仅edge映射主机80/443，PG/Redis无公开端口；API/Worker/BFF非root、read-only及资源限制。Caddy配置明确HTTPS跳转、HSTS/CSP/no-store；`/metrics`不反代到API，公网该路径不能读内部指标。真实宿主防火墙/SSH、DNS/TLS/headers仍需部署核对。

`production.env.example`只列公开选择项，不是完整可直接上线的服务秘密配置；生产identity/BFF env与秘密文件必须受控准备。metrics/签名/AEAD/SMTP文件RO挂载，应用UID10001及0600读取权限要实际匹配，不能改group/other权限迁就读取。Compose PG另按UID999备份目录权限准备，数据库密码和API私钥不共用权限。

原Dockerfile漏装`identity-keys`已修复；此命令必须随最终runtime新制品实测。历史[T22镜像digest](../T22/artifacts-summary.md)早于后续源码变化，只是当时本地review快照，不能用作最终发布摘要。最终提交须重新locked构建、验证六个命令、保存新checksum/digest和上版镜像；本报告不声称注册表发布或生产启动。

## RPO、保留与恢复

| 项目 | 本地证据/实现 | 必须完成的生产验证 |
|---|---|---|
| RPO≤15分钟 | Compose新增5分钟WAL切换，真实隔离PG显示5min并通过显式切段归档。 | WAL低流量切换约5分钟之外，独立存储复制/归档延迟预算须≤10分钟；监控失败/重试/最后成功归档，并实测事故到可恢复点窗口。参数本身不是RPO达标证据。 |
| 每日基础备份/≥14天 | 实际PG basebackup+verifybackup+age、独立根映射已验证。 | 调度、失败告警、不同故障域挂载、至少14天完整恢复链实际运行。保留窗口起点所需更早base及其连续WAL，禁止只按mtime删WAL。 |
| 新主机RTO≤60分钟 | [已有PITR](../T22/local-pitr-drill.md)命名点小探针正确，1.903秒；本次真实基础备份迁移路径恢复验证通过。 | 新主机恢复数据库/配置/签名与AEAD密钥，完整操作密码、TOTP、实体Passkey、OAuth和撤销，并从恢复开始到业务验证完成计时。小数据样本不能推定生产RTO。 |
| 非法备份 | 原跨路径checksum错误已修；当前只校验调用指定密文，拒历史路径/错清单/坏age/无效PG且不发布目标。 | 信任备份ID/版本清单，历史格式受控核对后转换；恢复演练保存失败并验证当前对象。checksum是辅助，age密文认证不替代可信来源、清单和key custody。 |

生产WAL链已提供具体配置：RO挂载脚本和运维核验age工具，独立WAL目录绑定，`archive_mode=on`/`wal_level=replica`/`archive_timeout=300s`/实际`archive_command`。工具与独立挂载必须存在，缺绑定源禁止自动创建。5分钟切段可能带来约4.5GiB/日未压缩WAL，14天约63GiB之外另计base/峰值；实际容量、报警和恢复链仍需生产测量。

计划中的`BACKUP_DESTINATION`现在实际供备份脚本读取，默认`base/`、`wal/`；`BACKUP_DIRECTORY`/`WAL_ARCHIVE_DIRECTORY`为对应显式覆盖，API不读取。路径变量不证明独立故障域。恢复私钥与签名/AEAD key另独立受控备份，不能仅备份加密数据而丢key。WAL锁异常遗留会令归档非零重试；确认无活动任务和旧对象状态后受控清理，不自动绕过锁覆盖。

## 指标秘密与告警

已逐行核对Config、core authorization、API与Worker handler。`METRICS_TOKEN_FILE`可选；生产文件必须普通文件、≤1024字节且无group/other权限，内容为43～512字符ASCII base64url，内存仅保留SHA256摘要。配置后必须唯一严格`Bearer `头，并常量时间比较摘要；错误/重复头拒绝，没有Cookie认证。未配置只允许真实loopback peer，私网Prometheus不能靠未配置token读取。

生产identity.env须明确`METRICS_TOKEN_FILE=/run/secrets/metrics-token`，API和Worker与Prometheus的`bearer_token_file`一致；RO挂载文件存在并不等于配置已启用。令牌不能进入日志、指标标签或证据。当前API/Worker聚合指标与[T22真实测试](../T22/metrics-summary.md)已验证，Promtool配置及9条应用规则语法通过；API不使用转发IP作为此鉴权的loopback来源。

Compose没有运行Prometheus/Alertmanager/Grafana；可用外部等价系统，但必须实际加入私网/采集秘密并验证告警接收。磁盘、证书、基础备份/WAL新鲜度的来源/规则及可达接收器仍需外部配置，不能把9条应用规则语法通过当覆盖全部告警。`IdentityDatabasePoolSaturated`当前门槛为32，调整`DATABASE_POOL_MAX`时须同步规则并验证告警。

## 密钥维护与回滚

`identity-keys public`仅导出已配置的当前/额外公开JWKS，不生成或打印私钥。额外公钥文件不含私钥，新公钥可先加入该集合供JWKS发布，再切私钥kid；真实分发到BFF、客户端缓存兼容、旧公钥≥12小时+2分钟仍待实测。普通ID Token继续检查exp，退出hint兼容不等于任意过期凭证可登录。

AEAD维护准确绑定user/purpose及BFF记录UUID/namespace，按用户/独立行锁事务；[实际演练](../T22/key-rotation-drill.md)覆盖当前全部加密用途、原文/TOTP一致、同用户坏tag回滚和幂等。生产要明确`--allow-production`、正确DB、owner-only版本map，不许删坏记录继续。

失败只回滚当前用户或对应独立批次；之前成功用户/BFF可能已经提交，不能说整库已回滚。切回旧active-kid必须保留包含新旧key的map，使已重加密记录仍可读；前版应用也必须能识别全部使用中kid。旧key保留直到所有当前记录及保留备份不再依赖。签名切回旧kid同样保留新旧公钥及独立私钥备份直到各自最后签发窗口结束。

## 发布顺序与限制

runbook明确配置校验→实际备份成功→显式迁移→启动→readiness→完整业务冒烟；迁移失败停止发布，schema兼容时才切上版应用，不能自动降库。真实失败中止/回滚及生产新旧镜像兼容尚待执行。SMTP的SPF/DKIM/DMARC与注册/恢复/安全通知实际送达，至少24小时及一个实际高峰观察仍无正式证据。

本次已修复审计发现的恢复对象选择、缺维护binary和WAL/变量配置问题，并留下必要复测；最终发布仍依据原关卡，恢复不可用/必要测试未执行及生产条件未验禁止放行。T24多副本/数据切换没有部署，本项目继续如实说明单机维护中断，不宣称HA。
