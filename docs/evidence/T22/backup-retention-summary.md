# T22 每日备份样例、真实 catalog 与保守保留

2026-10-09（Asia/Shanghai；以下UTC）。新增[保留工具](../../../infra/ops/backup-retention.mjs)、systemd[service](../../../infra/ops/systemd/identity-base-backup.service)/[timer](../../../infra/ops/systemd/identity-base-backup.timer)、[算法文件测试](../../../tests/ops/backup-retention.test.mjs)及[真实catalog演练](../../../tests/ops/backup-catalog.mjs)。没有改变原base/archive/restore源码或生产服务，没有安装或启动systemd timer，也没有自动删除真实备份/WAL。

最终纯测试6通过/0失败/0skipped、ESLint与diff-check0。`systemd-analyze verify infra/ops/systemd/identity-base-backup.service infra/ops/systemd/identity-base-backup.timer`退出0，证明样例语法，不证明每日调度或告警送达。PrivateTmp供basebackup临时写入，ProtectSystem strict只允许配置的备份挂载可写；生产账号/PG工具/挂载/0600凭据/失败通知仍需真实安装配置。

真实`node tests/ops/backup-catalog.mjs`最终18:39:43.496～18:39:55.118 UTC退出0，[原报告](backup-catalog-1791484795118.json)保存源SHA与三组检查。固定PG17.11及Node22.22.1镜像、age1.3.2使用任务专有network-none容器，无主机端口；Node只临时提取到任务目录，PG工具版本来自固定镜像，不提交第三方binary。所有任务容器/卷/解密数据/age身份结束清理，未停止或读写开发PG/Redis/SMTP。

- 实际两次pg_basebackup/stream WAL、pg_verifybackup、age及receipt生成。每个catalog调用真实restore-base校验/age解密/pg_verifybackup，提取backup_manifest/backup_label/pg_controldata；System-Identifier与源PG函数返回的十进制字符串准确一致，timeline1、真实Start/End LSN和16MiB段大小，密文hash绑定，catalog权限600。64bit ID不经浮点JSON数值转换。
- 选中真实最新密文再解密到新PGDATA、启动新专属PG，实际两条探针行存在，system ID/timeline匹配、实际重放/当前LSN及恢复时间写入受控proof。不是合成catalog的假恢复；这只是小probe元信息测试，完整身份恢复另有既有报告。
- 两份实际备份刚生成，没有14天前完成锚点。正常`plan`明确`no-complete-anchor-before-window`、候选为空，显式apply被拒且两备份仍存在；**没有把时间/proof变旧来完成实际删除，没有声称生产14天历史/RPO/RTO**。预期拒绝是本地安全行为通过，不是清理成功。

算法测试使用明确合成元信息/文件，仅证明保留决策：截止日锚点必须已完成，窗口起点时尚在backup的更新项不当锚点；保留锚点/所有较新/pin；proof须同对象/manifest/cluster且timeline/LSN/真实目标覆盖窗口；混合集群、分叉、错误/缺失proof、symlink/坏对象、活跃恢复与pin变化全部hold；keepDays<14拒绝。显式当前planhash/独占维护确认后仅删除更早基备份sidecars，WAL样本始终保留。不会把合成proof文件当实际恢复验收。

默认只plan；apply独占维护要求不代替实际停止并发写入，backup/restore管理员须遵守pin/active协议。失败可能遗留quarantine，错误文案明确需检查，不宣称错误后任何对象都未移动。工具不剪裁WAL、history、backup标识或私钥，因此没有总空间上限；容量告警和安全迁移存储需实际运维处理，不能按mtime补一轮危险WAL清理。

runbook已给owner-only配置、catalog/proof字段和命令、窗口/时间线/LSN算法、systemd未安装边界；T22 root runner接入需协调，纯6测试和独立真实catalog是新检查而非修改原23ops计数。正式每日调度、独立故障域、连续WAL至少14天实际保留链及新主机业务恢复仍待验收，T22/T23不因此放行。
