# PostgreSQL 基础备份、WAL 与恢复操作稿

使用固定PostgreSQL17客户端工具及age1.3.2。age为BSD-3-Clause，官方release Linuxamd64 SHA256 `cbe24006683f8eb669266162894b9a522a1af52f2665fbc63a4bb032ed26ac10`；下载必须核摘要，公钥recipient用于归档，私钥identity另独立受控存储。真实生产存储和权限尚未提供，本稿不是发布放行。

`infra/ops/base-backup.sh`要求PGPASSFILE owner-only、显式PGHOST/PORT/USER和独立BACKUP_DIRECTORY/AGE_RECIPIENT；pg_basebackup stream WAL、pg_verifybackup后才加密并发布checksum。数据库密码不放命令行或日志。每天调度成功记录，持续WAL至不同故障域存储，≥14天保留。调度/保留/可达告警需要实际配置验证，不能因脚本存在就声称已运行。

计划中的`BACKUP_DESTINATION`是备份任务读取的独立存储根；脚本默认映射到`BACKUP_DESTINATION/base`和`BACKUP_DESTINATION/wal`，显式`BACKUP_DIRECTORY`/`WAL_ARCHIVE_DIRECTORY`可覆盖对应目录。它们不由API读取，也不保证目录本身处于不同故障域。生产Compose把宿主`BACKUP_DESTINATION/wal`挂到PG的`/var/lib/identity-backup/wal`；配置文件只保存公开recipient和目录，age私钥独立保存。预建wal目录并使PG容器UID999可写，核对独立存储已实际挂载；准备经官方摘要核验、与主机架构匹配、可执行的`.local/production/tools/age`。Compose禁止自动创建缺失的工具/WAL宿主路径，不能以普通本地空目录替代独立存储。

生产PostgreSQL必须wal_level=replica、archive_mode=on、允许受控备份用户，archive_command调用archive-wal.sh `%p %f`。它仅接受PG WAL/历史/备份标識名，锁目录避免并发写同对象，已有对象需明文摘要匹配及密文checksum通过；错误非零交PostgreSQL重试，不能覆盖旧归档。写入后sync目录/文件，WAL目录仍需独立存储可用性监控。

WAL根目录现在必须预建、规范绝对路径且所有祖先不经symlink，运行归档账号拥有、group/other不可写；显式`WAL_ARCHIVE_DEVICE`为部署核验后的`stat -c %d`设备号。脚本开始、对象发布前与指针发布前再次核对，不再`mkdir -p`创建丢失挂载后的本地后备目录。生产Compose传该固定值；容器挂载的设备号须实际核验，不自动每次从当前路径更新，否则卸载无法被发现。相同设备的bind mount与故障域仍需部署验证，设备号不是独立备份证明。

成功对象目录包含`wal.age`、单行basename密文摘要、单行明文摘要及不可变`completion.json`。回执只在加密产物和清单已同步后写入，记录版本/PG段名/开始完成Unix秒/执行耗时/密文size/SHA；对象目录发布并sync后，全局`.archive.lock`保护原子`last-success.json`指针。文件必须普通、单链接、0600且非symlink，manifest只能精确`<SHA>  wal.age`，不能跟随清单里的外部路径。重试相同对象保留原完成时间，并重新同步全部已验证对象文件/目录、核验expecteddev后同步归档根，存在不能代表前次rename后的sync成功；较旧段不能回退指针，history/backup标签不刷新完整WAL新鲜度。历史无completion的完整旧对象可校验后重试成功，但不伪造新时间/回执；缺/损坏/future指针或对象拒绝，需运维核实修复并保留异常证据后重试，不静默以now覆盖。

监控见[host-metrics](host-metrics.md)：collector按固定WAL路径/设备、指针与对象回执/实际密文SHA校验后只输出聚合时间/执行耗时，无WAL段名标签。Worker从真实`pg_stat_archiver`输出archive_mode enabled、归档成功/失败计数与最后时间；查询/权限/连接失败让metrics失败，不制造健康零值。PostgreSQL真实NULL初始时间明确输出0，archive_mode off明确输出0并告警。单段加密/发布耗时不是ready积压或异地复制延迟；计数reset另有时间指标，不能把重启下降当归档成功。

Compose显式设置`archive_timeout=300s`，避免低流量时等待16MiB段写满才归档；有写入后目标段最长约5分钟切换，必须另将归档/独立复制延迟控制并监控在10分钟内，使最坏窗口预算不超过15分钟。归档失败会重试但本身不满足RPO，需监控`pg_stat_archiver`、最后成功归档时间和独立存储可恢复的新数据。每5分钟切段可能增加约4.5GiB/日的未压缩WAL，14天约63GiB，另计基础备份/峰值并实测容量。此配置和预算是待部署方案，尚无生产RPO证据。

空闲数据库的archive_timeout不会发送心跳，因此最后成功WAL超过15分钟只发warning要求调查空闲/积压，不直接标RPO已违反；真正最后失败晚于成功持续1分钟发critical。需要运维结合写入活动、`pg_wal/archive_status/*.ready`和独立复制确认实际积压/可恢复窗口，当前文件时间不能替代这类测量。WAL回执缺失/未来、采集停止和必要指标缺失均独立critical，单段发布超过5分钟warning。正式通知接收、时间同步、idle分类/积压观察和生产RPO仍须实际验收。

保留策略须保留至少14天内可恢复的基础备份及其连续WAL；窗口起点之前的必要基础备份及该备份以来WAL不能按mtime直接删除。每日调度、失败告警和删除前恢复链核对仍需运维系统实际执行。WAL锁目录在进程异常退出后可能遗留；确认没有活动归档任务且旧对象状态完整后再受控清理，不能自动绕过锁覆盖对象。

## 每日调度与保守基础备份保留

`infra/ops/systemd/identity-base-backup.service`和`.timer`是待安装示例：每天UTC02:00触发，Persistent补触发、最多5分钟随机延迟，同一个oneshot服务不重叠；服务UMask0077、受控identity-backup账号、owner-only `/etc/identity/backup.env`及独立挂载目录。安装前改为真实绝对目录/工具路径/账号，核对PG17客户端、age1.3.2与PGPASSFILE权限，配置已测试的OnFailure通知override和空间/备份陈旧告警。`RequiresMountsFor`不证明独立故障域，须实际检查挂载和故障域。`systemd-analyze verify`只验语法，不表示计时器已经运行、邮件通知可达或14天历史已经存在。timer不自动执行删除。

新`backup-retention.mjs`提供`catalog`、`plan`和显式`apply`。配置是0600受控JSON，包含规范绝对`backupDirectory`、`workDirectory`、`restoreScript`、`ageIdentityFile`、`ageBinary`、`pgControlData`与`keepDays`（最低14）；工作目录仅备份管理员可读，解密的临时数据用完删除。`catalog`读取当前真实完成receipt，调用现restore-base/age/pg_verifybackup，再提取PG manifest v2的System-Identifier（保留64bit十进制原值）、Manifest-Checksum、Start/End LSN/Timeline、backup_label START TIME和pg_controldata WAL段大小。它重新验证选中密文，原子写`<backup_name>.catalog.json`，不以mtime或文件名时间替代可信元信息；失败没有新catalog。

```sh
node /opt/identity/infra/ops/backup-retention.mjs catalog /absolute/owner-only-retention.json
node /opt/identity/infra/ops/backup-retention.mjs plan /absolute/owner-only-retention.json
```

保留计划按`cutoff=now-max(keepDays,14)天`判断。只支持已验证的同cluster单timeline，选择**完成时间不晚于cutoff**的最近基础备份作为候选锚点，保留该备份及全部更晚备份；备份在cutoff时尚未完成不能作为锚点。多cluster、timeline分叉/多范围、段大小矛盾、未知/缺失/损坏catalog、future时间或活跃/中断backup/restore标记均hold全部，不尝试推断分支可删。所有WAL段、`.history`/`.backup`和密钥始终保留，不实现自动WAL剪裁，因此存储空间可能持续增长、没有总空间上限；必须按真实增长配置容量告警，不能为腾空间直接删恢复链。

候选锚点必须还有0600 `<backup_name>.restore-proof.json`：真实恢复验证生成，绑定backup名/密文SHA/manifestSHA/System-Identifier，记录`pg_verifybackup_passed=true`、实际恢复timeline/replayed_lsn、恢复到达的真实时间点及完成时间、`postgres-recovery`/`business-validation`检查。目标时间必须覆盖cutoff，重放LSN至少包含该备份End-LSN；仅写true、合成旧时间、一次数据库启动或无摘要绑定不构成恢复证明。当前工具不会自动制造这份证明；由已经审查的真实恢复演练写入，无法证明则只能保留。正常新备份不足14天历史时`plan`明确hold且`apply`拒绝，不能改时间让删除变绿。

删除只在已确认备份/恢复任务静止的独占维护窗口执行，配置明确`exclusiveMaintenanceAcknowledged=true`；每个正在使用的备份预先写0600 `<backup_name>.pin`，恢复期间设置`.restore-active`，不能依靠清理工具自己的锁阻止另一个未配合进程。先审查plan和其`plan_sha256`，再显式：

```sh
node /opt/identity/infra/ops/backup-retention.mjs apply /absolute/owner-only-retention.json <exact-reviewed-plan-sha256>
```

工具重新验证实际对象、恢复proof和pin，计划变化拒绝。仅将更早的选中基础密文及同名sha/catalog/proof移入受控quarantine，同步后删除；不删除锚点、更新备份、pin对象、WAL、key或活动PGDATA。中断/错误可能留下quarantine，需核对并受控恢复，禁止自动移除残留后重试。此保守机制保持至少14天窗口所需锚点，但不能替代连续WAL/独立故障域/实际生产恢复验收。

本地验证见[备份catalog及保留证据](../evidence/T22/backup-retention-summary.md)。纯文件算法测试与真实PGcatalog测试分开，真实当前两份备份只证明元信息/还原和无14天历史时拒绝删除；不宣称已实际生产保留14天或达到RPO/RTO。

恢复前核对backup版本与checksum，通过restore-base.sh解密到新空目录并pg_verifybackup，绝不覆盖活动PGDATA。设置restore_command调用restore-wal.sh，指定recovery_target_time或已记录restore-point、recovery_target_action=promote，创建recovery.signal；启动前先恢复签名/AEAD/配置的独立受控备份。restore-wal只解密到临时目标，验证内容后替换，损坏/缺失返回失败。

新基础备份checksum只含单行SHA256与密文basename，搬迁后恢复只对调用参数指定的密文计算摘要，不跟随manifest路径。历史含绝对或目录路径的manifest明确拒绝：先在受控流程核对原manifest摘要、备份ID/可信清单及搬迁密文的真实摘要一致，再只把文件名改成对应basename；不得对未知对象直接重算摘要后宣称可信。缺失、多行、错误文件名/摘要、损坏age或无效PG备份均失败，解密/解包/verify失败不会发布目标PGDATA。checksum是完整性辅助，age认证不能替代可信备份版本清单或密钥保管。

本地必要边界测试已接入`npm run test:integration -- --task=T22`，使用隔离network-none临时PG17容器，不操作生产数据。默认工具路径`.local/security-tools/age/age`和`age-keygen`，支持`AGE_BINARY`/`AGE_KEYGEN_BINARY`明确覆盖；缺工具会失败。先按[工具来源及安装摘要](../evidence/T23/ops-repair-summary.md)核官方age1.3.2压缩包，提取二进制到忽略目录并设0755；不得提交工具、私钥或原始备份。此测试不代替新主机完整恢复和生产RPO/RTO。

演练记录必须保存backup ID/版本/校验、目标时间点、故障假定时间、实际丢失窗口与开始到业务验证完成耗时，并检查密码/MFA/Passkey/OAuth/撤销及邮件/监控。生产目标RPO≤15m/RTO≤60m，需要真实完整演练；[本地PITR记录](../evidence/T22/local-pitr-drill.md)仅证明小探针数据重放，不声明已达到生产目标。
