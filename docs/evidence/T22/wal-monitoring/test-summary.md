# WAL归档回执、真实数据库指标与告警

本地模块实际完成：耐久重试补充后统一运维Node测试38/38、原T22 API/Worker真实指标1/1、Promtool3.15.0总25条规则与31行为场景均退出0；无skipped。见[结果](test-result.json)、[运维测试](ops-tests.txt)、[Worker/API指标](worker-api-metrics.txt)、[规则测试](promtool-tests.txt)、[受测源码SHA](source-manifest.json)。没有停止共享开发PG/Redis/SMTP，不用5320，仅本任务临时PG与回环随机端口，结束清理。

`archive-wal.sh`现在要求预建归档根、规范绝对路径/祖先不经过symlink、归档用户拥有、group/other不可写和显式expected filesystem device。开始、对象发布和指针发布前核对，缺挂载不再mkdir本地fallback。目标目录与所有文件同样受控，manifest必须一行SHA及`wal.age`，不跟随外部路径；symlink、hardlink、多行/路径清单、错误明文/密文与回执均失败。所有权限以实际八进制位核对，未保留早期初稿误检查execute位的公式。

真实加密WAL和清单先sync，再写不可变completion回执并sync整个对象，发布目录后sync归档根；全局锁保护原子last-success指针。回执绑定段名、开始/完成时间、duration关系、size/SHA且拒未来时间。重试复用原完成时间，旧段不回退；history/backup标签不刷新完整段新鲜度。历史对象无回执可在原摘要验证后重试成功，不能伪造now；损坏/future指针明确失败并需受控修复。指针sync故障注入返回非零，已发布对象保持原回执，恢复重试时没有赋新时间。

collector检查WAL根与对象设备、指针和completion完全一致、普通单链接0600文件、真实密文size和流式SHA（120秒上限），不依赖mtime。输出只有固定`check="wal_archive"`与聚合最后时间/duration，无WAL段名、目录或身份标签。错误check0与本轮attempt时间，未来/zero/missing/stopped各有告警。

Worker从真实`pg_stat_archiver`读取archive_mode、成功/失败计数、最后成功/失败/统计reset时间。真实NULL初始时间明确0，SQL权限/连接失败让指标scrape失败，不用默认0冒成功；没有WAL名字段。专属真实PG测试比较Worker输出与数据库快照，在控制age失败时验证failed_count/last_failed_time增长、旧持久指针不更新，恢复后成功时间晚于失败；另REVOKE pg_stat_archiver访问使Worker实际失败，关闭pool也失败。临时权限只用于本任务PG，不触生产或共享权限。

真实PostgreSQL archive_command测试还验证：写入并pg_switch_wal后产生age加密16MiB段，解密长度和密文SHA正确；可控失败之后PG重试恢复并原对象receipt时间不变。测试用UID999:0700预建真实挂载根及实际expecteddev，没有放宽777目录或关闭加密校验。部署Compose新增明确WAL_ARCHIVE_DEVICE变量，离线模型检查仍保证工具/独立路径挂载与DB无公网。

新告警7条，原18条保持，总25：archive_mode未开/最新失败晚于成功critical；Worker必要指标缺失critical；持久回执指标缺失或时间无效critical；最后成功段超过15分钟持续5分钟仅warning要求调查；单段发布超过5分钟warning。原20场景+WAL11场景包含正常、duration300/301、最后失败/恢复、idle旧时间/for边界、zero/future/missing。空闲PG不会因archive_timeout发心跳，旧时间不能直接证明RPO违规；单段duration不是ready积压/异地复制延迟。生产需结合写入、ready积压和独立复制测实际可恢复窗口，当前没有测量生产backlog或RPO。

失败与修复如实记录：初稿权限公式错误/manifest与receipt绑定不足已在实测前改；真实Worker夹具曾在故障前仍有未完成成功归档导致指针变化，改为actual archive_status ready=0同步屏障；SQLx prepared statement拒绝多SQL权限设置，改逐语句执行；legacy夹具删除了当次latest回执后后续指针验证正确拒绝，改为明确受控有效B指针再准备旧legacy。最终没有放宽production校验或删除异常证据条件，统一38项源冻结后全部成功。

执行命令为：

```sh
AGE_BINARY=/absolute/verified/age AGE_KEYGEN_BINARY=/absolute/verified/age-keygen node --test tests/ops/base-backup.test.mjs tests/ops/wal-archive.test.mjs tests/ops/wal-receipt.test.mjs tests/ops/wal-worker.test.mjs tests/ops/production-config.test.mjs tests/ops/metrics.test.mjs tests/ops/alerts.test.mjs tests/ops/wal-alerts.test.mjs
cargo clippy --package identity-worker --all-targets --package identity-server --test t22_metrics --test t22_wal_metrics --locked -- -D warnings
```

定向ESLint、shell语法和diff检查0。T22集成ops阶段已要求精确38tests/38pass/0skip，missing脚本/工具不会跳过。生产独立存储设备/故障域、时钟同步、实际调度/保留、Prometheus/接收通知、数据库积压和复制延迟、生产RPO/RTO仍须验收；[运维方法](../../../runbooks/backup-recovery.md)与[指标稿](../../../runbooks/host-metrics.md)已写边界。

耐久重试补充：首版已经同步新发布对象，但existing/legacy/history/equal-newer指针早退不能以存在代表前次sync成功。现 `durable_object` 对已验证三文件/存在的completion、对象目录及再次expecteddev/根目录同步逐步显式失败；指针保留旧/newer分支另同步当前指针指向对象及last-success/root。测试分别模拟对象rename后rootsync失败、pointerrename后rootsync失败，同一故障重试仍非零，解除后仅补同步且原回执字节/时间不变；每个文件/目录/指针同步失败不能被后续sync吞掉，旧段保留新指针也验证这些错误传播。新增3个场景，WAL回执总11子测试；统一38项通过。
