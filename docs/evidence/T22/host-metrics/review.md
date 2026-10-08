# Receipt、主机指标和发布编排复核

2026-10-09（Asia/Shanghai）。只读核对当前 `base-backup.sh`、`collect-metrics.mjs`、`release.mjs` 与相关公开操作稿/证据，发现备份回执二次读取竞态后按授权完成最小修复。未运行容量负载、Firefox或真实容器发布演练，未停止共享开发服务。只审查当前本地制品，不放行生产。

发现与修复：旧发布逻辑先通过 `backupCompletion` 验证A回执和密文，再重新读取 `last-success.json` 记录名称/摘要。如果每日调度在两次读取之间原子发布B，事件记录可混合A完成时间与尚未由该调用校验的B名称/摘要；`.release-lock`不能约束独立备份调度。现在 `verifiedBackupReceipt` 返回同一次完整校验后的不可变回执，`backupCompletion`保留时间返回兼容接口；release直接复用该对象的时间/名称/摘要，不二次读取。受控文件测试在验证A之后立即写B回执并损坏B对象，断言最终release/current只记录A、B仍被真实完整性验证拒绝。

本次实际运行 `node --test tests/ops/release-order.test.mjs tests/ops/metrics.test.mjs tests/ops/alerts.test.mjs` 共16/16通过（release6、collector9、promtool1，含20规则场景），0失败/0skip；定向ESLint和diff-check退出0。输出见[回执竞态回归](receipt-race-tests.txt)，当前受测源码见[源码摘要](source-manifest.json)。之前真实容器release报告对应修复前源码，不能据其旧SHA宣称修复后完整容器演练已执行；根代理会在容量窗口结束后重新编排所需真实/完整回归。

核对结论：

- 基础备份只有 `pg_basebackup → pg_verifybackup → age → 密文/manifest发布与sync`完成后才原子写0600回执；字段仅版本/完成Unix秒/basename/密文size与SHA，无私钥或密码。失败不会更新上次完成时间，mtime不参与新鲜度判断。
- 回执/manifest/密文用 `O_NOFOLLOW`打开，要求普通单链接文件和group/other无权限；basename不接受路径，manifest只接受对应basename的一行摘要；真实密文大小/流式SHA及读取前后元数据一致才成功，未来超过60秒/零/非法时间拒绝。输出固定check/volume标签，未发现身份、token、目录或主机名标签。
- TLS使用默认信任或显式受控CA，`rejectUnauthorized:true`且正常主机名验证；DNS名称使用SNI，IP不伪装DNS SNI；握手有5秒期限。未发现关闭CA/主机名校验的成功路径，错主机/不可信/实际过期测试保留。
- 正式CLI要求配置`mode=production`与`--allow-production`相符；本地标签只能明确local-review；生产拒绝浮动镜像并要求五应用服务及全部Rust配置检查。备份前配置/镜像/facts校验，迁移失败阻止up/ready/smoke，回滚检查实际迁移摘要和在用AEAD kid范围且不执行迁移或数据卷删除。
- raw子进程诊断仅进入0700状态目录里的0600文件，公开报告检查为阶段/镜像/backup ID及密文SHA/迁移摘要。对公开T22证据和相关操作稿搜索私钥标记、age私钥、长token链接、Bearer/Cookie和秘密JSON字段并人工查看命中，未发现实际秘密值。历史本地路径/容器ID及脱敏传输错误是公开诊断，不当作秘密凭证。

必要边界：`O_NOFOLLOW`保护打开的最终文件，父级目录与挂载仍依赖受控部署权限；设备号检查不能证明独立故障域，同设备bind mount须另核验。每分钟采集的SHA读取成本/120秒限额、大备份容量、真实调度防重叠、备份保留/WAL连续性、Prometheus/exporter实际抓取、正式通知送达与生产RPO/RTO仍须验收。显式production参数是工具输入保护，不替代既有发布关卡。浏览器200%证据仍为根字体32px文本缩放，Linux Playwright Firefox只证明所测流程，不冒充桌面完整zoom、屏幕阅读器或五类实体浏览器矩阵。
