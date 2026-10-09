# T21～T23 本地完成定义审计

2026-10-09（Asia/Shanghai；证据时间均UTC）。只读审计起点`2253bfe11c4d09e41d17aeaacf7a0b9f5da536d8`，依据[plan.md](../../../plan.md)T21/T22/T23及第13节、[acceptance.md](../../../acceptance.md)必要案例。本轮不运行服务、故障或负载，不修改源码、任务状态和原关卡；此报告仅核对已存在证据及仍需完成的验收。

本地范围已经显著扩大：精确SQL/等待和密码队列观察、19任务集成manifest、Chromium/Firefox产品交互、宿主指标/告警规则、实际签名/BFF轮换、完整身份快照恢复及可执行发布/回滚均已有真实局部记录。当前新容量阶梯在执行，最终全面回归和最终生产镜像尚未与这些最新变更对齐，不能使用旧成功摘要自动推定完成。

## T21 性能、热路径与容量

| 要求 | 已有真实证据与覆盖 | 仍需实测/准确限制 |
|---|---|---|
| T21.01/.03/.04 安全种子与完整场次 | [四场记录](../T21/load-summary.md)：十万用户/授权、20客户端、真实token家族及受控刷新；每场120秒预热+900秒测量，300状态/100账号/5密码和355综合原counts/drop/错误满足本机场景。 | 主机为WSL2/Ryzen7700X 16logical/15.217GiB，同机负载器，与规定Linux8vCPU/16GiB/SSD不匹配；不能标参考硬件案例通过。 |
| T21.02/.06 密码队列与资源 | [队列指标](../T21/password-queue-metrics.md)明确waiting/held-slot/actual-running及等待bucket，不降64MiB/t3/p1/4任务/250ms参数；[带新观察的综合场](../T21/observed-mixed-summary.md)900秒全0、70快照、真实SQL完成/获取等待/PG事件及queue高水位。 | 此完整综合场在Redis连接复用修复之前；不能冒该修复后的性能结果。离散采样、原子快照、成功acquisition不等于全程精确峰值/无等待；持续SSD/主机调度和所有管理/Worker路径仍无完整证据。 |
| T21.05 精确SQL和N+1 | [SQL记录](../T21/sql-summary.md)：15条实际仓储SELECT形状带权威/time/version/归属/锁条件，真实typedbind EXPLAIN；HTTP实际完成事件：me1、introspection2、refresh13、密码22；本人sessions/grants limit1/20查询数相等。最新[短探测](../T21/sql-probe-2026-10-08T17-01-50-867Z.json)成功并保留源码/实际行数/观察。 | 短probe不是长测，不把setup单计划耗时当p95。N+1结论只限受测本人sessions/grants，不能声称全管理/outbox/Worker路由；[旧源码一致复核](../T21/sql-source-verification.json)有时间快照，后续变更须以新报告SHA为准。 |
| T21.07/.08 前端测量和失败 | [前端方法](../T21/frontend-measurement.md)五冷加载、固定390×844/4xCPU/网络及LCP/CLS/体积/反馈均实测；旧失败完整保留。 | 两匿名页面与实验室反馈不是全部账号/后台交互或生产RUM INP。当前产品修正后制品/性能是否变化需最终构建/必要针对测量确认，不造生产INP。 |
| T21.09 容量边界 | [边界说明](../T21/capacity-and-query-boundaries.md)明确355RPS为原工作负载下已测下界，写何时DB/API/控流决策，不编造最大值；[首次阶梯](../T21/capacity-first-analysis.md)如实记录710档错误、随后token池失效和更高档无独立解释。 | 当前`2253bfe`后的8分钟新阶梯仍执行，尚无完成报告；不预判成功或最大容量。每档60秒仅定位实验，不代替规定15分钟。修复后同机最大/参考机器/生产高峰都应按真实数据分别判断。 |

[T04共享Redis证据](../T04/redis-sharing-summary.md)另已实际证明连接复用、断开后503及恢复重建，安全Lua/失败关闭保留。它是连接行为/故障证据，不能单独证明吞吐修复。T21.09文档步骤可以完成，但T21整体不能据说明文件或短档推定通过。

## T22 运维与恢复

| 要求 | 已有实际本地覆盖 | 仍需完成的条件 |
|---|---|---|
| T22.01～.03/.11 镜像/权限/网络 | [1c0c9ea镜像](final-artifacts.md)6binary、UID10001/read-only/cap-drop/no-new-privileges、Caddy高端口实际启动；[tmpfs修复](../T22/tmpfs-fix.md)来自真实Compose错误后模型与专属部署验证，DB/Redis无公网。 | `1c0c9ea`镜像早于产品焦点/布局、队列/Redis等生产源码变化；最新提交须locked重建并实测真实Compose完整约束。正式主机防火墙/SSH/域名TLS仍未部署；单机中断不承诺HA。 |
| T22.04/.05 发布及回滚 | [编排/演练](../T22/release-orchestration-summary.md)：新备份实际核receipt/size/SHA才迁移；真实坏SQLxchecksum失败不up且原API容器不变；兼容文件比实际history摘要/usedkid，旧镜像回滚不降库并真实密码/session/退出冒烟。 | 本地演练只API和历史镜像，edge仅inspect，不能冒最新全套API/Worker/A-B/TLS发布。真实生产首次发布/全业务冒烟、正式旧版schema/key兼容与迁移窗口未执行；无自动down/drop。 |
| T22.06 指标与告警 | [宿主指标](../T22/host-metrics/test-summary.md)：真实statfs/预期device、正常CA/hostname/过期TLS拒、receipt与真实密文hash、原子textfile；18条Promtool规则，20行为场景含缺失/停止/阈值/恢复。API/Worker指标鉴权已有实测。 | 没有生产node_exporter/Prometheus/Alertmanager调度及通知到达；磁盘device检查不证明独立故障域，完整大备份SHA成本待容量。WAL新鲜度/归档延迟告警和正式collector接入须核对，不把规则测试当可达通知。 |
| T22.07 基础备份/WAL/14天 | [真备份边界](../T22/host-metrics/backup-receipt.txt)与[原PITR](../T22/local-pitr-drill.md)真实PG/age/verify/WAL；新completion receipt仅全部成功同步后发布，失败保留旧记录；搬迁/坏密文/坏PG拒，Compose5min切段。 | 仍无每日生产调度、独立加密存储和至少14天可恢复base+连续WAL链的实际运行。5min切段+≤10min复制为预算，不是RPO证据；没有借文件mtime冒成功。 |
| T22.08 完整身份恢复 | [完整恢复](../T22/identity-restore-summary.md)：真实密码/TOTP/虚拟WebAuthn、活跃session、live与已撤OAuth，复制DB与独立key后删除旧源，实际新PG恢复，新challenge签名/新RS256验证及新grant退出前true/退出后false。 | 它已补全小probe未验的本地身份流程，但数据规模小、同宿主任务容器、虚拟credential私有移植；不冒实体设备/独立生产主机。6.511秒仅当次小场景耗时，实际生产RPO≤15m/RTO≤60m仍需完整规模事故演练。 |
| T22.09 签名/AEAD轮换 | [签名轮换](../T22/signing-rotation-summary.md)真实HTTP新公钥先发布仍旧签→切新保旧→BFF缓存/一次JWKS刷新、过早删旧公钥负向、回滚双公钥、准确hint43320/43321与session43200边界；[AEAD演练](../T22/key-rotation-drill.md)全部当前用途/TOTP不变、坏tag用户rollback、幂等。 | 可控clock/本地BFF验证不等同实际12小时等待、生产多实例/客户端缓存分发和真实key备份/退役。必须保在用记录/保留备份所需旧key，不能因重加密完成立刻删恢复key。 |
| T22.10 SMTP | 原T05/07及产品两浏览器真实Mailpit投递/恢复，实际SMTP停机重试不丢动作。 | 正式SMTP账户、SPF/DKIM/DMARC、注册/恢复/安全通知送达及故障告警没有生产输入；Mailpit不替代。 |

[当前T22集成](../T22/integration.json)2026-10-08T16:59:57Z六组均0：metrics、AEAD、23项backup/WAL/config/hostmetrics/alerts、签名、完整恢复、发布兼容回滚。源码runner确实将它们接入，没有只写文档而漏执行。此局部结果早于最终全量，根仍要在最后提交核对全部必要套件。

## T23 完整回归、浏览器和制品

当前[suite manifest](../../../scripts/suite-manifest.mjs)包含**19个integration任务**（新增T21）和11个E2E任务；[T21入口](../../../tests/integration/T21.mjs)必跑observer4项/容量逻辑与实际SQL短probe；`test:accessibility`现同时跑T16和产品双浏览器。缺脚本、工具、数据库或未执行不得跳过并标绿。

[上次全量成功](../full-test/2026-10-08T13-41-49-384Z.md)是`bc742d1`的11阶段，已经覆盖E08/E12/E16三组合；**它早于当前新增SQL/指标/host告警/签名/完整恢复/release/Product/Redis变化**，不能用作`2253bfe`最终全量通过。新完整check/unit/integration/e2e/security/accessibility/build与最终制品SHA/镜像实际启动仍需根运行、留存失败和成功。

[双浏览器产品报告](product-accessibility/test-summary.md)实际Chromium153/Firefox155的10用例0、无skip，80组账号/后台四宽检查、20组文本200%缩放、键盘注册/邮件/密码/重置、真管理员恢复码/焦点/取消/无mutation、reduced-motion及ARIA。其修复焦点和长邮箱布局有真实回归。它不覆盖Firefox全部MFA/Passkey/SSO流程，不是官方稳定Chrome/Firefox/Safari/移动平台用户手测矩阵；root字号32px不等于完整桌面browserzoom，axe部分contrast incomplete不能写人工对比度通过，NVDA/VoiceOver/剪贴板/密码管理器/系统返回/实体手机仍需验。

正式品牌/合法文本、正式域名下Cookie/issuer/RP/回调/错误页、生产注册→邮件→密码/MFA/实体Passkey→A/B→全退出→禁用、监控/备份可用及至少24小时+一个实际业务高峰，仍按原T23.03～.09关卡。现有本地代码/报告能够审查，不自动代表第一版生产发布完成。当前acceptance/TASK_PROGRESS的部分“已完成本地最终全量”或旧T22只9规则/小probe文字应在新完整结果后同步准确当前范围，但本审计不直接改状态。

## T24 依赖与本地准备边界

T24明确前置T23、另验多副本/HA PG与Redis/滚动兼容/故障切换/新RPO-RTO/99.9%SLO；T23尚有真实外部验收未完成，不能借本机签名/共享BFF锁/回滚演练启动或宣称HA。仓库当前没有完整T24 HA Compose、数据层切换或独立SLO报告，现有单机runbook不是T24交付。

会话/授权权威DB、跨BFF共享锁、Worker租约、版本key/schema兼容这些既有实现与runbook可作为未来设计资料；若用户之后明确授权可另准备未部署的拓扑配置/故障runbook供审查。当前指令只要求完成本地T21～T23和本审计，**没有据此实施T24新配置、部署、故障或SLO测量**；先报告范围并依原前置推进，不自行新增feature。


## 当前结果更新

2026-10-08T19:40:55Z完整ffe3b98十一阶段全0，同提交新干净检出七阶段0、53制品SHA一致/身份秘密0。19集成任务包括T21/23ops/T22七组，11E2E、6安全以及12产品双浏览器流程均0，详[永久结果](../full-test/2026-10-08T19-22-22-758Z.md)。最新[制品](final-artifacts-20261009-focus.md)包含138edge焦点修复，runtime95生产输入同2c9、harness差异不入release编译。

先前审计指出的daily/保留代码缺口已补[systemd与catalog/保留工具](../T22/backup-retention-summary.md)，真实PG验证无14天anchor时拒清理；所有WAL保留且空间无上限。秘密结果/强认证连续焦点实际修复并两浏览器验证，原缺项closed。T05一次完整集成失败保留，三独立和最新完整复测0，未证实根因不冒修复。

此更新不把当前机器短/长压测或本地容器当规定参考硬件、真实域名/邮件/独立故障域、生产14天历史/通知到达/RPO-RTO或实体设备。T24[升级设计](../../runbooks/high-availability-upgrade.md)已补规划产物，实际部署/切换/SLO依赖T23正式验收，仍未实施。该结论属于此前基线快照。后续审查实际发现管理员客户端列表N+1（9→47次SQL），现正在批量修复复测；账号/管理六类交互已补测，WAL监测正在实现。当前新修改须独立最终全量与镜像验证；外部与人工必要项继续待验收。
