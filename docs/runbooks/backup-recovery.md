# PostgreSQL 基础备份、WAL 与恢复操作稿

使用固定PostgreSQL17客户端工具及age1.3.2。age为BSD-3-Clause，官方release Linuxamd64 SHA256 `cbe24006683f8eb669266162894b9a522a1af52f2665fbc63a4bb032ed26ac10`；下载必须核摘要，公钥recipient用于归档，私钥identity另独立受控存储。真实生产存储和权限尚未提供，本稿不是发布放行。

`infra/ops/base-backup.sh`要求PGPASSFILE owner-only、显式PGHOST/PORT/USER和独立BACKUP_DIRECTORY/AGE_RECIPIENT；pg_basebackup stream WAL、pg_verifybackup后才加密并发布checksum。数据库密码不放命令行或日志。每天调度成功记录，持续WAL至不同故障域存储，≥14天保留。调度/保留/可达告警需要实际配置验证，不能因脚本存在就声称已运行。

生产PostgreSQL必须wal_level=replica、archive_mode=on、允许受控备份用户，archive_command调用archive-wal.sh `%p %f`。它仅接受PG WAL/历史/备份标識名，锁目录避免并发写同对象，已有对象需明文摘要匹配及密文checksum通过；错误非零交PostgreSQL重试，不能覆盖旧归档。写入后sync目录/文件，WAL目录仍需独立存储可用性监控。

恢复前核对backup版本与checksum，通过restore-base.sh解密到新空目录并pg_verifybackup，绝不覆盖活动PGDATA。设置restore_command调用restore-wal.sh，指定recovery_target_time或已记录restore-point、recovery_target_action=promote，创建recovery.signal；启动前先恢复签名/AEAD/配置的独立受控备份。restore-wal只解密到临时目标，验证内容后替换，损坏/缺失返回失败。

演练记录必须保存backup ID/版本/校验、目标时间点、故障假定时间、实际丢失窗口与开始到业务验证完成耗时，并检查密码/MFA/Passkey/OAuth/撤销及邮件/监控。生产目标RPO≤15m/RTO≤60m，需要真实完整演练；[本地PITR记录](../evidence/T22/local-pitr-drill.md)仅证明小探针数据重放，不声明已达到生产目标。
