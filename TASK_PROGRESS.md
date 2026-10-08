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
| T13 | 通过 | 双BFF持久会话、维护SDK/SSO、共享刷新、真实故障/撤销及4E2E；[结果](docs/evidence/T13/test-summary.md) |
| T14 | 通过 | 真实初始化CLI、13管理API、强认证、最后管理员/因素与审计事务；[结果](docs/evidence/T14/test-summary.md) |
| T15 | 通过 | 官方设计研究、视觉稿与状态规范；用户明确采用方案并进入 T16；[用户确认](docs/evidence/T15/user-review.md) |
| T16 | 通过 | 设计组件、响应式布局、API/CSRF 客户端、可访问基础路由与保护提示；[结果](docs/evidence/T16/test-summary.md)、[UI 模块记录](docs/evidence/T16/ui-summary.md) |
| T17 | 待验收 | 6真实E2E与四宽20axe通过，前置T09实体设备未验收；[结果](docs/evidence/T17/test-summary.md) |
| T18 | 待验收 | 账号/会话/授权与统一认证4E2E通过；原前置T17仍待设备验收；[结果](docs/evidence/T18/test-summary.md) |
| T19 | 通过 | 四后台页面、13API、单次secret/权限、十万用户分页与3E2E；[结果](docs/evidence/T19/test-summary.md) |
| T20 | 待验收 | 六阶段安全/真实故障/新协议竞争通过，全量回归与T18前置未放行；[结果](docs/evidence/T20/test-summary.md) |
| T21 | 进行中 | 前端独立测量达标；后端300RPS首次dropped560未达，修负载器并复测 |
| T22 | 进行中 | 生产制品本地构建/配置通过，备份/监控/轮换及真实生产待验收 |
| T23～T24 | 未开始 | 第一版发布/真实设备与生产条件未满足，高可用后续独立阶段 |

T05 最终收尾记录：

- 最后两例 E2E 结果：2 passed / 0 failed，已同步 acceptance.md。
- T05 最终结论：通过（当前Linux真实开发服务及浏览器验收）。
- T05已提交推送 `7f736ac`；T06模块a4a1764已推送（首次commit_refs错误，重试后远端sha一致），后续遵循前置验收。

## 已实现能力

工程可构建、检查并在开发 Compose 启动。PostgreSQL、Redis、Mailpit、API、Worker 与三个前端均有实际开发启动验证；API liveness 与 readiness 区分依赖故障。

数据库已经具备用户、会话、OAuth 相关状态、一次性动作/挑战、认证器、管理员、outbox、审计与持久预认证实体。仓储明确用户→会话→授权→token/动作锁序，消费与后续写入同事务；摘要列不存可直接使用的令牌。

T04 实现本地弱密码检查、固定 Argon2id 参数与并发/等待预算、随机 token/恢复码、HMAC 与 AES-GCM 用户/用途绑定。浏览器状态变更验证精确 Origin 与数据库绑定 CSRF；只信任白名单代理，限流键不含原始邮箱/IP，依赖不可用失败关闭。

T05 已实现真实注册与邮箱验证：统一 202 不泄漏是否存在；新用户、验证动作、加密邮件 outbox 与审计共同提交；重发作废旧链接；用户点击 POST 才消费验证动作，成功不自动登录、不启用已禁用用户。Worker 提交租约后才发送 SMTP，失败保留重试，成功清除秘密参数。

前端采用已确认视觉方案及“统一身份中心”品牌，提供基础组件、布局、状态反馈、开发组件页和路由。T05 的注册/验证页面调用真实接口。账号/后台基础路由与前端 guard 不代表业务登录、管理权限或 SSO 已实现；安全必须由后续后端接口独立执行。

## 实际验证与限制

通过任务的命令、失败修复与复测结果以各证据目录为准。已经运行真实 PostgreSQL/Redis 事务与并发验证、依赖停止/恢复、SMTP 停止/恢复、浏览器键盘/对话框/表单与四种宽度检查；没有用 mock 数据库或假 SMTP 成功替代验收。

前端 transport/guard 的单元测试使用明确 fixture，仅验证客户端行为。浏览器 E2E 使用实际 API、数据库和 Mailpit。曾定位原生 fetch 的接收者错误导致 `Illegal invocation`，修复绑定后真实注册/验证单例通过；最后两例已真实通过。失败证据保留，不覆盖为成功。

T05 新 Cargo.lock 的在线 RustSec 公告更新失败已记录；随后 `cargo audit --no-fetch --json` 使用缓存公告库扫描 278 个依赖，0 漏洞、无警告。缓存 HEAD 为 `b0797f54ea5d1d5bc1266bff06e201d1c5e07dca`，提交时间 `2026-10-07T14:00:26+02:00`。这不是在线更新成功，也不代替 T20 全部安全扫描；[扫描说明](docs/evidence/T05/rustsec-cache.md)。

第一版尚未完成，当前不具备生产发布条件。密码登录、MFA、Passkey及授权码/OIDC端点已有真实模块验收；Passkey实体设备、刷新撤销、双BFF SSO、管理功能、完整产品流程、备份恢复与生产验收仍待对应任务。OpenAPI全量契约不能作为未实现端点可用的证据。

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

T11已推送83d1de3，T12已推送4f58e1c；当前T13双BFF与独立T14管理仓储并行实现。T17～T19产品流程整合依赖相应真实后端；T20～T23完成后才能评估生产发布。每个模块验收后提交推送，完整任务结束时生成最终测试总结。

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
