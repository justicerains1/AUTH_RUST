# 任务进度与接续记录

更新时间：2026-10-08（Asia/Shanghai）；功能验收于2026-10-07完成，仓库提交前检查于2026-10-08完成。实现基准：[plan.md](plan.md)、[acceptance.md](acceptance.md)。代码版本为本次初始化提交，由 Git 记录。仓库：https://github.com/justicerains1/AUTH_RUST ，分支 `main`。

用户要求本轮在 T05 收尾后停止推进，并初始化 Git、提交进度及推送代码。当前只完成已授权的收尾、验收记录与仓库发布；没有开始新业务任务。Git 推送是代码发布，不是生产部署。

## 当前任务状态

| 任务 | 状态 | 已完成范围与证据 |
|---|---|---|
| T00 | 通过 | 任务、步骤、依赖与文档基线；[审计记录](docs/evidence/T00/document-audit.md) |
| T01 | 通过 | Cargo/npm workspace、精确锁、六个 Rust 包、三个前端、开发 Compose、配置拒绝、健康检查、根脚本与 CI；[结果](docs/evidence/T01/test-summary.md) |
| T02 | 通过 | 架构、威胁、状态机、ADR、48 个 JSON API 与 11 个协议操作契约；[结果](docs/evidence/T02/test-summary.md)、[OpenAPI](docs/api/openapi.yaml) |
| T03 | 通过 | 迁移、约束/索引、仓储、统一锁序、原子消费、可控时钟与测试库隔离；[结果](docs/evidence/T03/test-summary.md)、[事务边界](docs/transactions.md) |
| T04 | 通过 | 邮箱/密码规则、受控 Argon2id、随机摘要、AEAD、CSRF/Origin、可信代理、Redis 原子限流、审计；[结果](docs/evidence/T04/test-summary.md) |
| T05 | 通过 | 注册、邮箱确认、重发、加密 outbox、Worker、SMTP 与真实页面；最终两例 E2E 2 passed / 0 failed；[结果](docs/evidence/T05/test-summary.md)、[浏览器证据](docs/evidence/T05/e2e.txt) |
| T06～T14 | 未开始 | 密码登录、找回/修改、MFA、Passkey、OAuth/OIDC、BFF SSO、管理 API |
| T15 | 通过 | 官方设计研究、视觉稿与状态规范；用户明确采用方案并进入 T16；[用户确认](docs/evidence/T15/user-review.md) |
| T16 | 通过 | 设计组件、响应式布局、API/CSRF 客户端、可访问基础路由与保护提示；[结果](docs/evidence/T16/test-summary.md)、[UI 模块记录](docs/evidence/T16/ui-summary.md) |
| T17～T24 | 未开始 | 完整认证/账号/后台产品页面、全面安全验收、性能、生产部署、发布与高可用 |

T05 最终收尾记录：

- 最后两例 E2E 结果：2 passed / 0 failed，已同步 acceptance.md。
- T05 最终结论：通过（当前Linux真实开发服务及浏览器验收）。
- 本轮停止点：T05 收尾；后续实施等待用户重新授权。

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

第一版所有业务能力尚未完成，当前不具备生产发布条件。正式密码登录、MFA、Passkey、OAuth token/discovery 实际端点、双 BFF SSO、管理功能、备份恢复与生产配置仍待对应任务。OpenAPI 全量契约不能作为这些端点可用的证据。

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

下一次用户明确继续后，核对本次记录，再从 T06 密码登录/会话开始，按前置顺序推进 T07～T14。T17～T19产品流程整合依赖相应真实后端；T20～T23完成后才能评估生产发布。当前按用户要求停止推进。
