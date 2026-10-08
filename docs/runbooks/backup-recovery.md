# PostgreSQL 基础备份、WAL 与恢复操作稿

使用固定PostgreSQL17客户端工具及age1.3.2。age为BSD-3-Clause，官方release Linuxamd64 SHA256 `cbe24006683f8eb669266162894b9a522a1af52f2665fbc63a4bb032ed26ac10`；下载必须核摘要，公钥recipient用于归档，私钥identity另独立受控存储。真实生产存储和权限尚未提供，本稿不是发布放行。

`infra/ops/base-backup.sh`要求PGPASSFILE owner-only、显式PGHOST/PORT/USER和独立BACKUP_DIRECTORY/AGE_RECIPIENT；pg_basebackup stream WAL、pg_verifybackup后才加密并发布checksum。数据库密码不放命令行或日志。每天调度成功记录，持续WAL至不同故障域存储，≥14天保留。调度/保留/可达告警需要实际配置验证，不能因脚本存在就声称已运行。

计划中的`BACKUP_DESTINATION`是备份任务读取的独立存储根；脚本默认映射到`BACKUP_DESTINATION/base`和`BACKUP_DESTINATION/wal`，显式`BACKUP_DIRECTORY`/`WAL_ARCHIVE_DIRECTORY`可覆盖对应目录。它们不由API读取，也不保证目录本身处于不同故障域。生产Compose把宿主`BACKUP_DESTINATION/wal`挂到PG的`/var/lib/identity-backup/wal`；配置文件只保存公开recipient和目录，age私钥独立保存。预建wal目录并使PG容器UID999可写，核对独立存储已实际挂载；准备经官方摘要核验、与主机架构匹配、可执行的`.local/production/tools/age`。Compose禁止自动创建缺失的工具/WAL宿主路径，不能以普通本地空目录替代独立存储。

生产PostgreSQL必须wal_level=replica、archive_mode=on、允许受控备份用户，archive_command调用archive-wal.sh `%p %f`。它仅接受PG WAL/历史/备份标識名，锁目录避免并发写同对象，已有对象需明文摘要匹配及密文checksum通过；错误非零交PostgreSQL重试，不能覆盖旧归档。写入后sync目录/文件，WAL目录仍需独立存储可用性监控。

Compose显式设置`archive_timeout=300s`，避免低流量时等待16MiB段写满才归档；有写入后目标段最长约5分钟切换，必须另将归档/独立复制延迟控制并监控在10分钟内，使最坏窗口预算不超过15分钟。归档失败会重试但本身不满足RPO，需监控`pg_stat_archiver`、最后成功归档时间和独立存储可恢复的新数据。每5分钟切段可能增加约4.5GiB/日的未压缩WAL，14天约63GiB，另计基础备份/峰值并实测容量。此配置和预算是待部署方案，尚无生产RPO证据。

保留策略须保留至少14天内可恢复的基础备份及其连续WAL；窗口起点之前的必要基础备份及该备份以来WAL不能按mtime直接删除。每日调度、失败告警和删除前恢复链核对仍需运维系统实际执行。WAL锁目录在进程异常退出后可能遗留；确认没有活动归档任务且旧对象状态完整后再受控清理，不能自动绕过锁覆盖对象。

恢复前核对backup版本与checksum，通过restore-base.sh解密到新空目录并pg_verifybackup，绝不覆盖活动PGDATA。设置restore_command调用restore-wal.sh，指定recovery_target_time或已记录restore-point、recovery_target_action=promote，创建recovery.signal；启动前先恢复签名/AEAD/配置的独立受控备份。restore-wal只解密到临时目标，验证内容后替换，损坏/缺失返回失败。

新基础备份checksum只含单行SHA256与密文basename，搬迁后恢复只对调用参数指定的密文计算摘要，不跟随manifest路径。历史含绝对或目录路径的manifest明确拒绝：先在受控流程核对原manifest摘要、备份ID/可信清单及搬迁密文的真实摘要一致，再只把文件名改成对应basename；不得对未知对象直接重算摘要后宣称可信。缺失、多行、错误文件名/摘要、损坏age或无效PG备份均失败，解密/解包/verify失败不会发布目标PGDATA。checksum是完整性辅助，age认证不能替代可信备份版本清单或密钥保管。

本地必要边界测试已接入`npm run test:integration -- --task=T22`，使用隔离network-none临时PG17容器，不操作生产数据。默认工具路径`.local/security-tools/age/age`和`age-keygen`，支持`AGE_BINARY`/`AGE_KEYGEN_BINARY`明确覆盖；缺工具会失败。先按[工具来源及安装摘要](../evidence/T23/ops-repair-summary.md)核官方age1.3.2压缩包，提取二进制到忽略目录并设0755；不得提交工具、私钥或原始备份。此测试不代替新主机完整恢复和生产RPO/RTO。

演练记录必须保存backup ID/版本/校验、目标时间点、故障假定时间、实际丢失窗口与开始到业务验证完成耗时，并检查密码/MFA/Passkey/OAuth/撤销及邮件/监控。生产目标RPO≤15m/RTO≤60m，需要真实完整演练；[本地PITR记录](../evidence/T22/local-pitr-drill.md)仅证明小探针数据重放，不声明已达到生产目标。
