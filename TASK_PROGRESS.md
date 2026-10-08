# 任务进度与接续记录

更新时间：2026-10-08（Asia/Shanghai）。实现基准：[plan.md](plan.md)、[acceptance.md](acceptance.md)。各模块版本由下方记录及 Git 历史追踪。仓库：https://github.com/justicerains1/AUTH_RUST ，分支 `main`。

用户已恢复推进：按文档依赖继续，每完成一个模块并通过相应测试后提交推送。最终运行完整测试；若仍有失败，按用户要求推送并提供总结，失败不标通过。Git推送不等于生产部署。

## 当前任务状态

| 任务 | 状态 | 已完成范围与证据 |
|---|---|---|
| T00 | 通过 | 任务、步骤、依赖与文档基线；[审计记录](docs/evidence/T00/document-audit.md) |
| T01 | 通过 | Cargo/npm workspace、精确锁、六个 Rust 包、三个前端、开发 Compose、配置拒绝、健康检查、根脚本与 CI；[结果](docs/evidence/T01/test-summary.md) |
| T02 | 通过 | 架构、威胁、状态机、ADR、48 个 JSON API 与 11 个协议操作契约；[结果](docs/evidence/T02/test-summary.md)、[OpenAPI](docs/api/openapi.yaml) |
| T03 | 通过 | 迁移、约束/索引、仓储、统一锁序、原子消费、可控时钟与测试库隔离；[结果](docs/evidence/T03/test-summary.md)、[事务边界](docs/transactions.md) |
| T04 | 通过 | 邮箱/密码规则、受控 Argon2id、随机摘要、AEAD、CSRF/Origin、可信代理、Redis 原子限流、审计；[结果](docs/evidence/T04/test-summary.md) |
| T05 | 通过 | 注册、邮箱确认、重发、加密 outbox、Worker、SMTP 与真实页面；最终两例 E2E 2 passed / 0 failed；[结果](docs/evidence/T05/test-summary.md)、[浏览器证据](docs/evidence/T05/e2e.txt) |
| T06 | 通过 | 密码登录、本人账号/设备及派生授权撤销；真实并发与2E2E通过，[证据](docs/evidence/T06/test-summary.md) |
| T07 | 通过 | 密码找回/修改、近期认证与安全通知；真实API/SMTP、并发及2E2E通过，[结果](docs/evidence/T07/test-summary.md) |
| T08 | 通过 | TOTP、单次恢复码和强认证；真实并发/重放/完整浏览器验收，[结果](docs/evidence/T08/test-summary.md) |
| T09 | 待验收 | 实现/真实PG策略及2个虚拟签名E2E通过，外部真实设备阻塞；[记录](docs/evidence/T09/test-summary.md) |
| T10 | 通过 | 受管理client/授权同意、严格PKCE和浏览器绑定；真实API/2E2E通过，[结果](docs/evidence/T10/test-summary.md) |
| T11 | 通过 | 原子授权码交换、RS256及旧公钥、scope Userinfo、成熟OIDC互操作与真实并发/故障；[结果](docs/evidence/T11/test-summary.md) |
| T12 | 通过 | 刷新重放家族撤销、权威introspection/RP确认、scope限制、真实并发/故障及2E2E；[结果](docs/evidence/T12/test-summary.md) |
| T13 | 通过 | 双BFF持久会话、维护SDK/SSO、共享刷新、真实故障/撤销及6E2E；[结果](docs/evidence/T13/test-summary.md) |
| T14 | 通过 | 真实初始化CLI、13管理API、强认证、最后管理员/因素与审计事务；[结果](docs/evidence/T14/test-summary.md) |
| T15 | 通过 | 官方设计研究、视觉稿与状态规范；用户明确采用方案并进入 T16；[用户确认](docs/evidence/T15/user-review.md) |
| T16 | 通过 | 设计组件、响应式布局、API/CSRF 客户端、可访问基础路由与保护提示；[结果](docs/evidence/T16/test-summary.md)、[UI 模块记录](docs/evidence/T16/ui-summary.md) |
| T17 | 待验收 | 6真实E2E与四宽20axe通过，前置T09实体设备未验收；[结果](docs/evidence/T17/test-summary.md) |
| T18 | 待验收 | 账号/会话/授权与统一认证4E2E通过；原前置T17仍待设备验收；[结果](docs/evidence/T18/test-summary.md) |
| T19 | 通过 | 四后台页面、13API、单次secret/权限、十万用户分页与3E2E；[结果](docs/evidence/T19/test-summary.md) |
| T20 | 待验收 | 六阶段安全/真实故障/新协议竞争通过，全量回归与T18前置未放行；[结果](docs/evidence/T20/test-summary.md) |
| T21 | 待验收 | 前端及四场15min本机实测阈值通过；原参考环境/完整容量及T20前置未验；[结果](docs/evidence/T21/test-summary.md) |
| T22 | 待验收 | 本地制品/指标/PITR/AEAD维护通过；生产恢复/SMTP/独立存储与告警未验；[记录](docs/evidence/T22/test-summary.md) |
| T23 | 待验收 | 干净检出/完整回归/最终制品正在收尾；设备与生产放行条件未满足，[清单](docs/evidence/T23/remaining-verification.md) |
| T24 | 未开始 | 后续高可用独立阶段，不阻塞第一版；尚未实现或部署 |

## 当前交付与验证范围

邮箱注册/确认、密码登录/恢复/修改、TOTP/恢复码/Passkey、OAuth/OIDC、双 BFF SSO、账号与管理后台均已实现。模块级真实数据库/HTTP/浏览器测试已经执行，具体版本和失败修复以各任务证据及 Git 历史为准。

本地生产制品、指标、加密备份/WAL 和版本化密钥维护已实现；不能据此宣称正式生产部署、邮件送达或恢复目标达标。实体 Passkey 与五类浏览器人工矩阵、参考机器完整性能/容量、生产域名/主机/SMTP/独立备份/告警仍待真实验收。

2026-10-08 第一轮最终完整测试已真实完成，11 阶段全部执行，整体退出 1（unit/integration/e2e/security 失败）；原失败报告永久保留。正常流程有效期、Redis 恢复等待、后台 MFA 夹具与三条源码摘要误报已修复；备份迁移对象校验、runtime 密钥命令和 Caddy 权限问题也已修复并推送。第二完整回归正在运行，不能提前标全部完成。

## 并行模块分工

| 模块 | 交付分工 |
|---|---|
| 根整合 | 任务依赖、原文修订、跨模块检查、验收状态、前端 API 集成、Git 初始化与发布 |
| 工程/前端与 API 契约 | 三前端初始化、OpenAPI/契约、仓储/Clock、基础 UI、浏览器安全边界、注册/验证服务、fetch 排错 |
| 架构/基础设施与密码/Worker | 官方规范与威胁/ADR、Compose/镜像、迁移、视觉研究、密码/加密、SMTP Worker |
| 脚本与真实验收 | 跨平台根脚本、文档/OpenAPI 检查、真实数据库/HTTP/故障测试、Playwright 与脱敏证据 |

代理通过任务与模块并行，具体边界各有负责人；密码/协议/认证事务没有通过互相覆盖代码完成。所有代理共享工作区，最终状态由根整合核对。

## 文档校正

已经同步处理规划与实现的具体差异：

- 原文“本轮仅文档”保留为历史交付背景，当前完成状态由真实代码与验收更新。
- OpenID authorize 与 RP logout 同时支持规范要求的 GET/POST form 入口；入口只建立流程，最终同意/退出另行验证 Origin/CSRF。
- 明确 Passkey 列表及详情路径、TOTP enrollment confirm 完整路径，保持契约与路由表一致。
- 未知 token_type_hint 按 RFC 7009/7662 忽略；协议错误与中文 JSON API 错误分离。
- 补充数据库权威预认证实体与 CSRF 生命周期，避免获取 CSRF 时丢失已绑定流程。
- 校正早期认证任务与后置页面验收的隐性依赖：提供调用真实服务的最小页面，完整产品体验在 T17/T18 整合；后续全面发布关卡保留。
- 明确局部 `test:security -- --task=Txx` 验证不代表未完成的全量安全套件通过。

具体来源与影响见 [T00 审计](docs/evidence/T00/document-audit.md)、[T04 校正](docs/evidence/T04/document-revisions.md)、[分阶段验证](docs/evidence/T04/phase-validation.md)。

## 外部验收条件

用户已明确回复生产域名、部署主机、SMTP服务和独立备份“暂未准备”。本地开发及独立测试继续；T22/T23的真实生产部署、送达与恢复演练保持待验收，不能通过模拟结果放行。

## 独立协议输入模块

T10.03的纯协议输入子模块已先行完成并推送a4320d2；4个Rust规则测试与Clippy通过。它不创建授权或签发令牌，T10完整授权/同意随后完成并推送d055641。

## 完整测试编排模块

已增加 `npm run test:full`：完整安装、各类检查/测试与构建逐项执行，任何失败仍继续并生成 `TEST_SUMMARY.md`。3个行为测试和ESLint实际通过；这是编排模块验收，不是系统完整测试通过。原始诊断只保存在`.local/full-test/`，不进入Git。

## 重跑与接续

在仓库根目录按 [README](README.md) 准备 Rust、Node/npm、Docker Compose 与 OpenSSL。首次本地启动：

```text
npm ci
npm run dev:secrets
npm run dev:up
```

已有 `.local` 秘密时不要重新生成或覆盖，持久卷与数据库密码需一起保留。检查与当前局部验收：

```text
npm run verify:docs
npm run verify:openapi
npm run test:tooling
npm run check
npm run test:unit
npm run build
npm run test:integration -- --task=T03
npm run test:integration -- --task=T04
npm run test:integration -- --task=T05
npm run test:security -- --task=T04
npm run test:accessibility
npm run test:e2e -- --task=T05
npm run dev:down
```

`dev:down` 保留开发卷。未实现的任务套件不能返回伪成功；测试库与临时 schema 清理不会指向 production。不要把 `.local`、密码、邮件链接、私钥、token、Cookie 或含这些值的浏览器 trace 提交 Git。

所有模块提交与真实证据由以下接续历史追踪，当前状态以本文件顶部总表和 acceptance.md 为准；以下“下一任务”文字保留当时阶段背景，不能用作当前尚未实现的判断。最终完整总结保存在 TEST_SUMMARY.md。

## 最近完成：T11

授权码交换与RS256/Discovery/JWKS/Userinfo通过真实API、外部OIDC客户端、独立JOSE验证；十并发仅一个令牌家族，签名失败完整回滚，PG故障503，Cookie不可替代Basic。[完整结果](docs/evidence/T11/test-summary.md)。当前discovery仅公布实际授权码能力，T12完成后扩展。

## CI 恢复修复

T01→T03的顺序失败已在本地真实复现并修复：API就绪早于Docker健康探针，恢复结束需等待容器healthy。顺序复测两项退出0；新增脱敏CI诊断保留原检查和失败状态。Windowscheck具体原因仍待下一远端诊断，[结果](docs/evidence/tooling/ci-recovery/test-summary.md)。

WindowsOpenSSL构建前置已按实际MSVC失败诊断修复并推送3a06d16，严格核对官方预装版本及开发库；Windows远端check/unit/build仍等待实测。CI恢复修复39f5b83后远端T03已过，集成继续到T07浏览器失败，仍保留未通过状态。

## 最近完成：T12

刷新轮换/重放审计、scope限制、即时撤销、RP确认、真实PG/Redis停机和2个浏览器流程通过。锁内时间修复覆盖普通退出/密码变化，真实竞争验证及受影响回归通过；[记录](docs/evidence/T12/test-summary.md)。

最新远端CI：a499520/run37711498834的Linux、Windowscheck/unit/build及当前T01～T12集成全成功；此前失败保留。[实际结果](docs/evidence/tooling/ci-recovery/remote-passed.md)。当前T13双BFF开发环境已真实启动，模块SSO/并发/故障仍在独立验收。

## 最近完成：T13

同包A/B/A2真实SSO与PG跨实例单次刷新、状态故障503、本地撤销失败清会话、平台确认退出和callback篡改验证4E2E通过；52前端测试、BFF6单元及完整检查/构建通过。开发10服务真实healthy与secret受限初始化已验证。[结果](docs/evidence/T13/test-summary.md)。下一T14草稿恢复后接HTTP与真实权限/并发测试。

## 最近完成：T14

真实双CLI并发初始化、13后台API强认证、最后管理员及因素并发保护、即时撤销/secret轮换/审计rollback通过；T08/T09回归0，T09设备仍待验收。f44afd7截至T13远端Linux/Windows及全部集成CI成功，run37718112029。下一进入T17完整认证产品页，再T18账号/同意及T19管理UI，[T14记录](docs/evidence/T14/test-summary.md)。

## T19 查询子模块

前置T14通过后，T19.01/.05的服务端精确邮箱/状态过滤及最多31天审计窗口已独立真实验证并推送17da861；cursor绑定过滤摘要，非法/重复与跨过滤复用拒绝。管理UI仍未开始，此子模块不标整体T19通过。[记录](docs/evidence/T19/filter-test-summary.md)。

## 最近完成：T17

T17实现与6真实认证E2E、20个四宽页面axe、键盘/降低动态通过；原依赖T09实体设备未验收，T17整体保留待验收。旧T05/T06/T07/T09各2E2E回归0，50身份与12演示UI测试及构建检查通过。T09实体设备/全量发布限制仍保留，下一T18账号整合与T19后台界面。[记录](docs/evidence/T17/test-summary.md)。

## 最近完成：T18代码与浏览器案例

账号与安全/会话/grant/consent整合、统一近期认证、恢复码显示保存清理与虚拟Passkey真实验证4E2E通过。任务依赖T17仍待真实设备，T18保持待验收；代码及相应检查通过可提交。T20.01依赖/秘密扫描子模块已推1eb8b0f，不代表全面安全验收。[记录](docs/evidence/T18/test-summary.md)。

当前T19三真实后台浏览器案例通过，测试库十万用户分页与0008索引实际EXPLAIN ANALYZE通过；原顺序扫描与迁移缓存证据保留。T21前端独立五次冷加载测量已运行达局部LCP/CLS/JS阈值，不代表后端压测通过。T22生产制品正在编写，本地用户生产资源未准备，真实上线/恢复待验收。

## 最近完成：T19

三真实后台案例与十万用户索引分页通过，旧7套E2E回归通过；58身份/12演示单元及构建检查0。下一T20完整威胁/故障/协议/ZAP安全测试；T21前端独立测量通过、后端压测准备；T22制品配置只本地验证，生产资源仍未准备。[结果](docs/evidence/T19/test-summary.md)。

## T20安全子套件与完整编排

实际security六阶段、当前故障复测与新增绑定竞争检查通过，生产构建ZAP仅信息提示无中高危；已修CSP/Worker审计DTO及私有工具lint路径。全量integration/E2E按固定manifest真实执行，失败继续，缺文件失败；最终完整test:full尚待长压测结束才运行，不能先标通过。

T21状态检查正式复测已通过：15分钟270000请求精确300RPS、drop0/业务错误0，p95=4ms/p99=5ms；原两次完整调度失败保留。账号/密码/综合三场继续前先独立运维演练短空档，本机环境与原8vCPU参考不同仍如实记录。

## T22 本地制品与运维模块

固定非root镜像/Caddy、指标与规则语法、age归档边界、独立小probePITR及全AEAD重加密CLI真实验证通过。原生产资源未准备、完整恢复/每日独立备份/可达告警/签名分发仍待验收，T22不放行。[记录](docs/evidence/T22/test-summary.md)。

## T21完整本机负载实验

四个2min预热+15min场景实际目标counts全完成、drop0/业务错误0，mixed319500请求；原失败修复与报告保留。当前硬件不同参考基准且完整容量/前置未验，任务保持待验收。负载已释放，随后开始用户要求的完整test:full与cleancheckout验证。[结果](docs/evidence/T21/test-summary.md)。

## 最终测试收尾（2026-10-08）

- 正常认证测试夹具有效期修复 `7a6e203`；完整 unit 实际 Rust58/身份59/A6/B6 全过。
- 运维修复 `946dbb0`：搬迁备份只校验指定对象、缺陷备份拒绝发布、runtime包含identity-keys、生产WAL归档链及独立目录映射；T22三阶段及12项ops实际通过。
- 故障恢复/MFA夹具修复 `319f30c`：真实PONG等待、只使用未消费恢复码；T13四E2E/T19三E2E通过，生产重放保护保留。
- Caddy严格权限修复 `1c0c9ea`：高端口服务删除无用file capability，保持cap-drop ALL及no-new-privileges；最终提交镜像正在独立验证。
- [第一轮完整失败](docs/evidence/full-test/2026-10-08T12-14-09-892Z.md)及[干净首轮失败](docs/evidence/T23/clean-checkout/2026-10-08T12-11-26-363Z.md)保留；第二全套回归和全新干净构建正在运行，完整通过前不宣称结束。

本地复现完整回归前需按 README 准备开发服务、固定浏览器和安全工具，包括 age1.3.2；缺工具/缺必要套件会真实失败。Git推送不代表生产部署。最终结束当前工作后停止自动推进后续生产/HA任务，等待已记录的外部验收条件。


干净检出收尾已通过：[319f30c](docs/evidence/T23/clean-checkout/test-summary.md) 七阶段全部0、53制品SHA一致、无身份秘密。与最终1c0c9ea应用/锁/测试源码相同，差异仅Caddy Dockerfile及证据。最终[本地镜像](docs/evidence/T23/final-artifacts.md)已经从1c0c9ea构建并按完整生产权限约束实测通过，保存新OCI摘要；未registry发布或生产部署。第二完整回归仍进行中。


第二轮完整集成T13原组合故障案例触发60秒整体预算，拆为独立故障场景并保持原断言/时限后，后续E2E六案例实际通过（12:51:54～12:52:48Z），修复提交f5e7640。生产代码未改；该轮旧集成失败仍保留，最终将以修复提交再执行整套。
