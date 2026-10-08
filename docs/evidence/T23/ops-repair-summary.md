# T23 运维缺陷修复与独立验证

2026-10-08 12:33 UTC，Linux/WSL2，Node22.22.1，Docker Compose2.40.3；审计起点为`add1400da832c66bfe7fed240b6804b49e769333`，本报告对应随后未提交的运维修复文件，最终提交/制品由根发布记录提供。不声称已部署生产或完成T22/T23整体验收。

## 修复

- `base-backup.sh`原checksum包含旧存储路径，`restore-base.sh`跟随该路径，搬迁后可能失败或校验另一对象。现在checksum仅含basename，恢复显式计算调用参数指定密文的SHA256；缺失、多行、错名、错摘要及历史路径格式均拒绝。历史格式只能先受控核对摘要/版本，再转换basename，不能自动信任重算的清单。
- 基础备份先在独立存储临时目录加密并同步文件，checksum最后发布，缺manifest的对象不算完成。恢复先解密/解包/`pg_verifybackup`到目的文件系统临时目录，通过后才发布不存在的目标；失败不发布PGDATA，已有目标不改。
- `Dockerfile.prod`增加`identity-keys`复制，解决release编译后未装入runtime的遗漏。最终镜像重建、六命令实际运行和新digest另由根执行；此处没有冒称已通过镜像测试。
- 备份任务正式支持`BACKUP_DESTINATION`，默认映射`base/`与`wal/`；保留显式目录覆盖。env示例和runbook说明映射、独立故障域和权限，API不读取备份变量。
- 生产Compose显式设置`wal_level=replica`、`archive_mode=on`、`archive_timeout=300s`及实际归档命令，挂载只读脚本/运维核验age工具和独立WAL目录，禁止自动创建缺失的绑定源。runbook补归档延迟预算、14天完整恢复链和密钥回滚兼容要求。

## 实际测试

命令：

```sh
AGE_BINARY=/tmp/auth-rust-t22-age/age \
AGE_KEYGEN_BINARY=/tmp/auth-rust-t22-age/age-keygen \
node --test tests/ops/base-backup.test.mjs tests/ops/wal-archive.test.mjs tests/ops/production-config.test.mjs
```

工具为[T22已核验age1.3.2](../T22/age-source.md)。基础备份测试使用固定PG17.11镜像`sha256:3645570cccdfa447589da9f57dd740faa29b30938e861289a5574b6ca6b03826`；实际临时容器命名`identity-backup-files-<UUID>`、network-none，无主机端口。测试仅删除自己的容器、自动卷与临时目录，不访问或停止共享开发PG/Redis。秘密和原始备份临时生成、权限0600，不写入证据。

根随后将同一核验工具安装在Git忽略的`.local/security-tools/age/age`及`age-keygen`，T22 integration默认使用该位置，仍支持显式`AGE_BINARY`/`AGE_KEYGEN_BINARY`。工具缺失/错误或任何ops案例失败均非零，12项检查已接入完整T22 runner，不能静默跳过。安装需先核对官方Linuxamd64发布压缩包SHA256 `cbe24006683f8eb669266162894b9a522a1af52f2665fbc63a4bb032ed26ac10`，提取age/age-keygen到该忽略目录并设0755；实际提取二进制SHA256分别为`eb7dd1b518f0a307c99cd97782623c5321da049154b04acd2d98d21aa7bc9b2c`、`0a0009db842259d6717f7eeb30acb6b90d2a2eb924c6acd0a0db0ca1f1537899`。本次临时路径与默认路径摘要已实际逐一一致。此工具安装不包含生产age私钥，生产挂载位置另为`.local/production/tools/age`。

最终实际输出：**12 passed、0 failed、0 skipped、0 cancelled，退出0，9.333秒**。该计数含3个父案例与9个基础备份子案例。

| 范围 | 实际结果 |
|---|---|
| 真PG归档 | PG显示`archive_timeout=5min`；写入真实表后显式`pg_switch_wal()`，经真实`archive_command`生成age密文，`pg_stat_archiver.archived_count>0`。没有等待5分钟计时验证，也没有生产延迟测量。 |
| 真基础备份 | 实际`pg_basebackup --format=plain --wal-method=stream`、`pg_verifybackup`、tar/age；新manifest精确相对basename。 |
| 搬迁与错误对象 | 源目录不存在时迁移新路径恢复成功；旧路径存在其他对象时指定新对象仍成功；指定新对象损坏即失败，即使旧路径存着有效原对象也不能放行。 |
| 清单与密文 | 历史路径、缺清单、多记录、错文件名、错摘要拒绝；篡改密文且重算checksum仍被age拒绝，目标不存在。 |
| PG数据与已有目标 | 有效age密文包含无效PG备份时`pg_verifybackup`失败，临时目录清理且不发布目标；已有目录及哨兵内容保持。 |
| WAL文件边界 | canonical根映射、重复归档幂等、原文恢复一致；不同原文/非法WAL名拒绝，损坏密文恢复失败且无目标。此案例使用合成WAL字节，另有上方真实PG归档。 |
| Compose离线模型 | 派生副本只把缺失production env标为optional，不提供秘密；保留真实`--project-directory=infra`。断言实际脚本/age source路径、PG参数、拒自动创建绑定源、DB/Redis无外部端口和仅edge80/443。没有生产Compose启动。 |

独立`node node_modules/eslint/bin/eslint.js tests/ops --max-warnings 0`、四脚本`sh -n`及`git diff --check`均退出0。未重复执行共享全量套件；根统一完整测试与最终制品。

## 保留失败与复测记录

- 初版Compose验证因真实production env缺失失败；测试改用明确派生副本。中间副本的env_file对象形态不合法再次失败，改为列表。随后断言canonical JSON保留显式false失败：Compose默认省略false；改为检查不能为true，并补真实解析source路径。每次失败均返回非零，最终模型通过，未降低生产配置要求。
- 增补真实PG归档时第一次ready误命中initdb的临时Unix socket，随后psql提示数据库正在停止，退出1。改为只检查最终服务器TCP127.0.0.1 readiness。
- 第二次归档因合成fixture父目录0700导致PG UID999不可遍历，实际归档未产生，10 tests中8pass/2fail（包含失败父案例），退出1。只调整测试公开目录0755/归档目录可写，密钥/密码仍0600；全部边界重跑12/12通过。生产runbook同样要求预建目录、PG权限和真实独立挂载，不声称容器可自动解决这些条件。

本次没有新PITR至命名恢复点测试，也没有全账户/实体Passkey/生产签名分发验证；已有小样本PITR仍见[T22记录](../T22/local-pitr-drill.md)。RPO≤15分钟/RTO≤60分钟、每日调度/14天保留、独立存储、生产恢复、告警送达仍待真实验收。
