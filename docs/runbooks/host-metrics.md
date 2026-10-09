# 单机磁盘、证书和完整备份指标

`infra/ops/collect-metrics.mjs` 补充 T22.06 的本地运维采集制品。它检查实际磁盘、HTTPS issuer TLS 和最新完成基础备份，原子发布 Prometheus textfile。它不启动 exporter、Prometheus、调度或告警接收服务；生产环境未准备，部署与实际告警送达保持待验收。

先核对真实数据/独立备份存储已经挂载、故障域与权限正确，再按 [公开配置示例](../../infra/ops/metrics.env.example) 准备受控服务配置。`OPS_DATA_DIRECTORY` 为宿主 PG 数据目录，`OPS_BACKUP_DIRECTORY` 为基础备份目录（默认备份任务使用 `BACKUP_DESTINATION/base`）。路径必须为规范绝对路径。使用 `stat -c %d <真实目录>` 确认文件系统设备号后写入 `OPS_DATA_DEVICE`/`OPS_BACKUP_DEVICE`，不可自动每轮更新预期值。路径丢失、设备不匹配、容量越界或读取失败都输出检查失败；该检查可发现不同设备的挂载卸载，不能单独证明存储处于独立故障域，同设备 bind mount 仍需部署核验。

`ISSUER` 必须是无用户信息的固定 HTTPS origin。TLS 握手使用正常 CA/主机名验证并限制5秒，读取经验证叶证书的到期时间；不可信 CA、错主机或过期证书不能报成功。可通过 `OPS_TLS_CA_FILE` 显式提供受控企业 CA；不设置则使用运行时默认信任 CA，不使用关闭 TLS 验证的环境变量。证书/文件路径和主机名不会进入指标标签或错误日志。

`base-backup.sh` 在 `pg_verifybackup`、age加密、密文与SHA清单发布并同步磁盘全部成功后，原子写同目录的 `last-success.json`。严格字段为 `version:1`、`completed_at`（Unix秒）、`backup_name`（固定日期/PID的密文basename）、`ciphertext_bytes`、`ciphertext_sha256`。后续失败不更新该记录。采集器检查最新记录、相对basename清单、实际密文size与流式SHA-256，不回退到较旧对象，不把mtime或文件名时间当作成功；`touch`不刷新备份完成时间。备份目录不可被group/other写，记录、清单和密文必须是单链接普通文件、group/other无读写执行权限，不接受symlink。

完整加密产物及checksum匹配证明这次受控备份脚本完成，不证明恢复成功、生产RPO/RTO、WAL连续性或14天保留。每日备份失败会因未更新完成记录触发超时，脚本退出非零仍应由实际调度系统即时通知；该采集器不冒充备份调度的实时退出结果。每轮仅核验最新完整密文，SHA检查限120秒；实际大备份读取成本需实测。运行时长/容量不满足一分钟调度时必须调整资源/验证流程并重新核验阈值，不能跳过SHA后宣称同等完整性。

建议由有权读取备份与目录的受控服务账号每分钟运行一次；Node需符合项目版本（当前22.22.1）。调度禁止重叠实例。`OPS_METRICS_FILE`必须是`.prom`且父目录由运行账号拥有、group/other不可写，输出0600/0640权限按受控group供node_exporter读取。采集失败仍原子发布本轮`success=0`与当前attempt时间，退出1；配置或发布失败不覆盖上次文件，由陈旧告警发现。不要把备份私钥或数据库密码提供给采集器。

```sh
node /opt/identity/infra/ops/collect-metrics.mjs
```

现有 node_exporter（或等价受控textfile服务）需读取该目录，Prometheus使用固定`job_name: identity-ops`抓取并加载 [18条总告警规则](../../infra/ops/identity-alerts.yaml)。这个仓库尚未部署这些服务；正式环境必须限制指标访问、核验目标配置、实际抓取、制造受控失败并确认通知到达。文本文件被继续抓取时`up=1`不能说明采集器仍运行；规则使用指标里的attempt Unix秒识别停止超过5分钟及未来/零时间。每个固定check与必要领域指标均有缺失检测，避免单项缺失被整组存在掩盖。

| 指标/告警 | 固定标签及阈值 |
| --- | --- |
| 每项采集success/attempt时间 | `check=disk_data/disk_backup/issuer_tls/base_backup`，失败1分钟；停止超过5分钟、未来超过60秒或零时间，持续1分钟 |
| 磁盘size/available bytes | `volume=data/backup`；warning低于10%或1GiB，持续5分钟；critical低于5%或512MiB，持续1分钟 |
| issuer证书notAfter | 无动态标签；剩余不足14天warning持续5分钟，不足3天或已过期critical持续1分钟 |
| 完整基础备份完成时间 | 无动态标签；超过24小时、零值或未来超过60秒，持续5分钟 |
| exporter/必要指标缺失 | 固定`job=identity-ops`；不可达、缺抓取目标、任一必需指标缺失，持续1分钟 |

生产时钟同步、实际每日备份时长、空间预算、证书续签窗口和通知接收配置都需验证。规则中的warning/critical可能同时发出，应在正式Alertmanager中评审抑制策略，不能因本地规则单测通过就填“告警可达”。

本地真实验证命令（缺工具即失败）：

```sh
node --test tests/ops/metrics.test.mjs tests/ops/alerts.test.mjs
AGE_BINARY=/absolute/verified/age AGE_KEYGEN_BINARY=/absolute/verified/age-keygen node --test tests/ops/base-backup.test.mjs
```

`PROMTOOL_BINARY`可显式指向经过摘要核验的Promtool3.15.0，默认`.local/security-tools/prometheus-3.15.0.linux-amd64/promtool`。测试检查真实statfs、真实TLS（可信、错主机、不可信、过期）、文件/权限/原子输出及promtool指标格式；规则测试覆盖20个健康/阈值/for/缺失/停止/未来/恢复场景。`base-backup`测试用network-none隔离PG17并真实备份/验证/加密与失败旧记录保持，不停止共享开发依赖。

WAL扩展配置为`OPS_WAL_DIRECTORY`及`OPS_WAL_DEVICE`，与实际已挂载归档根一致。collector新增固定`check="wal_archive"`、`identity_ops_wal_archive_completed_timestamp_seconds`/`identity_ops_wal_archive_duration_seconds`，不输出WAL名/路径。严格核对规范目录及祖先、预期设备号、指针与对象不可变回执完全一致、manifest仅`wal.age`、实际size/流式SHA，错误输出check0与当前attempt；不从mtime或重试推造新完成时间。

Worker新增真实`pg_stat_archiver`聚合指标（job identity-worker）：enabled、archived_total、archive_failures_total、last_archived/last_failed Unix秒、stats_reset时间。NULL初始时间=0是明确缺历史状态，SQL权限/连接错误不会变成零成功。总规则现25条，原20个host场景另新增11个WAL场景：失败未恢复/disabled/missing/future/duration边界和idle旧观察仅warning。最后成功段超过900秒持续5分钟发“待调查”warning，不将空闲数据库直接判RPO；最后失败晚于成功持续1分钟critical。采集器停止仍由attempt时间检测，pg_archive_timeout300s不是空闲心跳。

## 采集调度与本地接收链

新增[collector service](../../infra/ops/systemd/identity-ops-metrics.service)和[timer](../../infra/ops/systemd/identity-ops-metrics.timer)供部署评审。服务通过受控EnvironmentFile运行Node，ProtectSystem=strict/NoNewPrivileges/PrivateTmp，只允许写textfile目录；同一systemd unit运行中不会启动第二实例，140秒超时覆盖并行两个120秒摘要检查及TLS。每分钟Calendar调度与Persistent示例已用systemd-analyze校验，未安装生产调度。部署前修订实际service用户、metrics可读group、受控备份读取权限、挂载路径和Node绝对路径。

[Prometheus配置](../../infra/ops/prometheus.yaml)已加入固定job identity-ops与私网identity-ops-exporter:9100，并连接identity-alertmanager:9093。部署需要实际私网DNS/服务与ACL；[Alertmanager模板](../../infra/ops/alertmanager.yaml.example)需要替换为受控receiver。模板没有虚构生产收件人或发送通知。

[真实本地接收链](../evidence/T22/monitoring-chain/test-summary.md)已执行collector→node_exporter→Prometheus→Alertmanager→回环HTTPreceiver。检查实际健康scrape、缺WALreceipt的pending、原for:1m后firing、恢复resolved；规则原文未改，实验scrape/evaluation改1s而生产15s。本地密文输入明确合成，实际age/TLS/statfs和SHA校验不能冒成PG备份/RPO。正式发送到运维接收端、实际调度/抓取故障、静默/抑制与空间/时钟/14天恢复历史继续实测。
