# T22 本地磁盘、证书及基础备份告警证据

本次补齐原9条应用规则没有覆盖的磁盘/证书/基础备份采集与告警制品，最终本地测试通过；18条总规则（新增9条host规则）通过真实Promtool3.15.0语法和20个行为场景。采集器+规则Node测试10/10、真实隔离PG基础备份测试11/11均退出0，无skipped。[结果](test-result.json)、[受测源码SHA](source-manifest.json)、[采集与规则输出](collector-rules.txt)、[真实备份输出](backup-receipt.txt)、[规则语法](promtool-rules.txt)可复查。

`collect-metrics.mjs` 实际读取statfs并核对预期设备号；严格CA/主机名验证HTTPSissuer（包括可信成功、错误主机、不可信、实际过期证书）；检查受控完成receipt/规范相对basename清单/实际密文size及SHA，原子写固定标签Prometheus文本并由promtool check metrics解析。失败仍写本轮检查失败与attempt时间；配置/发布失败非零且不泄露配置。没有数据库密码、备份私钥、账号/域名/路径/备份ID指标标签。

`base-backup.sh` 只有真实pg_basebackup、pg_verifybackup、age加密、两文件发布和sync成功后才原子写`last-success.json`；PG连接失败保留上一完成记录并退出非零。测试实际恢复对应对象并保留原损坏、搬迁、多行/错误清单等边界。采集器测试另证明更新mtime不刷新完成时间，future/路径注入/size/hash不一致、缺失/损坏/符号链接文件拒绝，不退回旧备份冒充新成功。

20个PromQL场景覆盖健康基线、exporter0/完全缺失、单项check及领域指标缺失、失败attempt仍更新且恢复、up1但静态文件停止、future/zero采集时间、两个独立volume、warning/critical与精确5%/10%边界、绝对空间阈值、证书14/3天及已过期、backup24小时和for临界前后、zero/future/缺完成时间。每个场景同时断言全部9条host规则，防止意外额外告警。

过程中真实失败保持清楚：最初健康证书场景在24小时后自然进入14天阈值，测试样本修为每分钟仍在精确14天边界；TLS夹具监听通配IPv6遭端口冲突，改为明确IPv4回环；尝试以openssl负days生成过期证书被工具拒绝，改为显式历史notBefore/notAfter，真实TLS仍拒绝已过期可信证书。最终不降低TLS验证或放宽告警断言。

运行命令：

```sh
node --test tests/ops/metrics.test.mjs tests/ops/alerts.test.mjs
AGE_BINARY=/root/code/rust/auth_rust/.local/security-tools/age/age AGE_KEYGEN_BINARY=/root/code/rust/auth_rust/.local/security-tools/age/age-keygen node --test tests/ops/base-backup.test.mjs
node node_modules/eslint/bin/eslint.js infra/ops/collect-metrics.mjs tests/ops/metrics.test.mjs tests/ops/base-backup.test.mjs tests/ops/alert-fixtures.mjs tests/ops/alerts.test.mjs --max-warnings 0
.local/security-tools/prometheus-3.15.0.linux-amd64/promtool check rules infra/ops/identity-alerts.yaml
```

以上退出0，`git diff --check`0。仅启动并清理本任务network-none临时PG容器及本地回环TLS服务器，不停止共享开发依赖。实际配置、阈值、运行成本和使用方法见[运维指标操作稿](../../../runbooks/host-metrics.md)。

没有部署node_exporter/Prometheus调度或Alertmanager、没有实际通知到达，没有生产SMTP/DNS/独立备份故障域与14天保留证明。设备号检查不能独立证明故障域，同设备bind mount仍需人工部署核验；完整密文SHA每轮最大120秒读取成本需在真实容量验证。每日备份失败只有最后成功时间自然超时告警，实际调度仍须即时通知退出非零。该完成记录不证明恢复/RPO/RTO或WAL连续性。T22.06及正式T22-OPS-04继续待验收，不以本地制品和规则单测冒充生产告警送达。
