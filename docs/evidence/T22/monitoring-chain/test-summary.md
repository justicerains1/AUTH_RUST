# 本地监控采集、告警和恢复接收链

[最后实际报告](2026-10-09T01-31-05-820Z.json)退出0；四个阶段均实际执行，缺持久WAL回执后的告警在61.354秒后由Prometheus触发，经Alertmanager投递到本机回环HTTPreceiver，恢复校验回执后收到resolved且ALERTS清除。源码摘要执行前后匹配。只用了本任务回环随机端口和独立进程，没有发送外部联系。

使用真实node_exporter1.12.1 textfile collector、Prometheus3.15.0、Alertmanager0.34.1；[官方来源及下载摘要](tool-sources.json)核对GitHub release资产，解包LICENSE均Apache-2.0。默认路径为`.local/security-tools/node_exporter-1.12.1.linux-amd64/node_exporter`、`.local/security-tools/prometheus-3.15.0.linux-amd64/prometheus`、`.local/security-tools/alertmanager-0.34.1.linux-amd64/alertmanager`及amtool。工具缺失真实失败，不跳过。工具可执行SHA另在报告。

数据源是collector真实statfs、受信任本地TLS、age真实加密对象与严格SHA/完成回执。明文明确合成运维监測输入，并非PostgreSQL备份或WAL；它用于证明传输与告警动作，不证明备份可恢复、归档连续或生产RPO。真实PG故障/恢复另见[WAL模块](../wal-monitoring/test-summary.md)。

[测试](../../../../tests/ops/monitoring-chain.mjs)读取实际生产Prometheus配置，替换实验回环地址、受控规则/凭据路径，并将15秒抓取/评估改1秒；原identity-alerts.yaml规则及for:1m没有修改。私有receiver仅接收IdentityOpsCheckFailed+wal_archive，其他尚未部署API/Worker的告警在实验中没有外发目的地。健康success1实际scrape；移除WAL指针使collector发布success0，先pending且零通知，再firing；恢复原合法指针后success1、告警解除且resolved实际送达。

增加受控collector service/timer：每分钟Calendar、同unit不重叠、ProtectSystem=strict/NoNewPrivileges/PrivateTmp、只写textfile目录、140秒deadline。systemd-analyze verify与Calendar实际退出0，未安装生产service或观察实际调度。Prometheus现在有identity-ops target和Alertmanager地址；Alertmanager模板没有真实接收人，部署需替换受控私网接入及实际receiver。

首运行失败保留：合成age产物继承默认0644权限，严格collector拒绝不安全对象。夹具改为0600后完整链通过，不放宽生产权限；[首次失败](2026-10-09T01-21-43-742Z-failure.json)和[第一次完整成功](2026-10-09T01-23-07-161Z.json)保留。最终又验证了从生产alerting配置派生的完整链，没有额外改变规则阈值。

命令：

```sh
node tests/ops/monitoring-chain.mjs
systemd-analyze verify infra/ops/systemd/identity-ops-metrics.service infra/ops/systemd/identity-ops-metrics.timer
```

本地链已接入T22必跑阶段，缺工具/抓取/pending/firing/resolved任何一步失败使整个阶段非零。Node ESLint与diff检查0。正式私网服务、生产调度、SMTP/实际运维通知到达与24小时观察仍待真实环境，不以本机receiver替代生产接收方验收。
