# Rust 统一身份验证系统：步骤验收文档

文档版本：1.0  
编写日期：2026-10-07  
关联计划：[plan.md](plan.md)

> 初始交付仅为文档；用户已于 2026-10-07 授权开始代码实施并要求多 agent 分模块并行。当前T11模块通过，后续按前置顺序继续，逐项记录真实执行结果。原第 11 节保留为历史文档交付记录，不能作为系统实现验收。

## 1. 验收规则

1. 先确认前置任务通过，再执行对应案例。依赖缺失填“阻塞”，不能把 mock 当真实验收。
2. 每个案例记录代码版本、环境、执行时间、命令退出码或操作结果和证据。
3. 原文中的“预期”不能复制到“实际”当证明。实际字段必须来自运行结果。
4. 任何涉及认证、CSRF、MFA、撤销、并发的案例不能只看页面截图；必须验证后端状态。
5. 并发测试使用同步屏障/可控事务交错，证明至少一个指定竞争窗口，不能只连续调用两次。
6. 过期测试用时钟或数据库时间构造，避免依赖长等待；真实时间性能测量除外。
7. 所有证据脱敏，不提交完整 token、Cookie、密码、二维码种子、私钥和恢复码。
8. 自动化检查失败返回非零；缺失测试不能跳过并显示成功。
9. 新增缺陷必须关联任务。High/Critical、关键流程和数据恢复失败阻止发布。
10. 用户设计确认只验收视觉方案，不替代安全、可访问性和性能测试。
11. 生产基础设施及真实设备未提供时，继续独立任务，不能提前标记发布通过。
12. 每任务必要案例全过才写“通过”；无法执行的关键项目不允许标为不适用。
13. T05～T09/T10任务内提供调用真实后端的最小功能性页面进行Playwright验收，复用已通过T16；完整产品页面T17/T18再整合。T07尚未有完整MFA/refresh端点时，用真实数据库因子/授权状态及仓储同步屏障验收保留/撤销边界，T08/T12/T20补实际端点竞争；T11 discovery仅声明已实现能力，T12补齐最终能力。后置E2E在T20/T23完整回归，不放宽发布关卡。

## 2. 环境与证据格式

### 2.1 任务验收头

每次执行补齐：

```text
任务 ID：
代码 commit：
验收人/执行者：
日期时间：
OS / Rust / Node / npm / Docker：
PostgreSQL / Redis / Mailpit / Browser：
配置环境：development / test / production
测试数据库名称：只写名称，不写连接密码
机器 CPU / RAM / Disk：
命令及退出码：
证据路径：
```

环境基线和性能指标以 plan.md 第 8.4 节为准；性能机器不同则报告实测，不直接声明参考基线达标。

### 2.2 证据目录

后续任务创建 `docs/evidence/Txx/`，文件示例：

- `environment.md`：版本与资源限制。
- `commands.txt`：命令、时间、退出码；脱敏。
- `test-summary.md`：案例、实际结果、失败原因。
- `screenshots/`：无秘密的页面截图。
- `performance-summary.md`：阈值、测量条件、摘要、原始输出位置。
- `defects.md`：复现、影响、修复与复测。

大型报告可作为 CI artifact 保存，记录可定位版本和 SHA-256；验收不能仅链接会失效的临时 URL。

### 2.3 允许状态

任务：未开始 / 进行中 / 待验收 / 通过 / 阻塞。  
案例：未执行 / 通过 / 失败 / 阻塞。  
T24 不阻塞第一版；其他关键任务不得用“后续再做”释放关卡。

## 3. 任务验收总表

| 任务 | 内容 | 当前状态 | 前置 | 证据 |
|---|---|---|---|---|
| T00 | 落地文档与任务基线 | 通过 | 无 | 本文第 11 节文档检查记录 |
| T01 | 初始化工程、依赖与开发启动 | 通过 | T00 | docs/evidence/T01/test-summary.md |
| T02 | 威胁模型、状态机与接口契约 | 通过 | T01 | docs/evidence/T02/test-summary.md |
| T03 | 数据库迁移、仓储与事务原语 | 通过 | T02 | docs/evidence/T03/test-summary.md |
| T04 | 安全基础、密码服务、限流与审计 | 通过 | T03 | docs/evidence/T04/test-summary.md |
| T05 | 注册、邮箱验证与 outbox 邮件 | 通过 | T04 | docs/evidence/T05/test-summary.md |
| T06 | 密码登录、账号查询和会话撤销 | 通过 | T05 | docs/evidence/T06/test-summary.md |
| T07 | 找回密码、密码修改与安全通知 | 通过 | T06 | docs/evidence/T07/test-summary.md |
| T08 | TOTP、恢复码与近期认证 | 通过 | T06、T07 | docs/evidence/T08/test-summary.md |
| T09 | Passkey 注册、登录和管理 | 待验收 | T06、T08 | docs/evidence/T09/test-summary.md |
| T10 | 受管理客户端与授权/同意事务 | 通过 | T06、T02 | docs/evidence/T10/test-summary.md |
| T11 | 授权码交换、ID Token、Discovery 和 Userinfo | 通过 | T10 | docs/evidence/T11/test-summary.md |
| T12 | 刷新轮换、Introspection、撤销与 RP 退出 | 未开始 | T11 | 待提供 |
| T13 | 两个 BFF 演示应用与接入指南 | 未开始 | T12 | 待提供 |
| T14 | 管理员初始化与管理 API | 未开始 | T08、T10、T12 | 待提供 |
| T15 | Awwwards 研究、设计稿与视觉规范 | 通过 | T00；可与后端并行 | docs/evidence/T15/user-review.md |
| T16 | 前端基础、组件和可访问路由 | 通过 | T15、T02 | docs/evidence/T16/test-summary.md |
| T17 | 品牌、注册登录、MFA 与密码恢复页面 | 未开始 | T05～T09、T16 | 待提供 |
| T18 | 账号安全、会话、授权及同意界面 | 未开始 | T12、T16、T17 | 待提供 |
| T19 | 管理后台 UI | 未开始 | T14、T16 | 待提供 |
| T20 | 安全、协议、并发和故障全面验收 | 未开始 | T13、T18、T19 | 待提供 |
| T21 | 性能、容量和交互优化 | 未开始 | T20 | 待提供 |
| T22 | 单机生产、监控、备份和密钥轮换 | 未开始 | T20、T21 | 待提供 |
| T23 | 第一版发布与总体验收 | 未开始 | T00～T22 全通过 | 待提供 |
| T24 | 后续高可用升级 | 未开始 | T23；不阻塞第一版 | 待提供 |

## 4. 逐任务验收

每个案例的操作须按对应任务步骤完成。以下表格的“实际/结果/证据”在真实执行后填写；T00 将通过本轮只读检查记录。

### T00 — 落地文档与任务基线

**前置：** 无。  
**关联实现：** plan.md 的 T00.01～T00.05。  
**产物核对：** 本轮交付 plan.md、acceptance.md；后续创建 docs/evidence/、ADR 模板和文档检查脚本。  
**验证命令/方式：** 本轮：只读文档结构检查。后续：npm run verify:docs。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T00-DOC-01 | 工作区文档已保存 | 检查任务编号、依赖、目录及状态说明 | 两文档任务集合均为 T00～T24，功能未标记通过 | 实际文件读取验证：25 任务、199 步骤一一对应，T01～T24 未开始 | 通过 | 第 11 节检查记录 |
| T00-DOC-02 | 两份文档 | 核对已选择方案与本次文档范围 | 明确文档交付与未来代码实施的区别 | 两文件仅描述规划，未将未来命令和功能写成运行成功 | 通过 | 第 11 节复核记录 |

**实现子步骤检查：**

- [x] T00.01：保存两份中文 Markdown，确保任务编号 T00～T24 完整且相互链接。
- [x] T00.02：列出用户选择、当前环境、范围边界、外部输入和真实完成状态。
- [x] T00.03：按任务填入前置条件、实现子步骤、接口或数据变化、测试操作、预期与证据。
- [x] T00.04：后续实现启动时创建 ADR 模板和证据目录；初始功能状态全部未开始。
- [x] T00.05：实现 verify:docs 检查任务覆盖、重复编号、依赖引用、相对链接和状态值；本轮用只读检查完成同类文档验证。

**验收记录：**

- 代码版本：待填写。
- 环境与时间：待填写。
- 命令退出码：待填写。
- 失败/阻塞项：待填写。
- 修复与复测：待填写。
- 任务结论：通过（仅限本轮文档产物与一致性，后续证据目录及 verify:docs 脚本随 T01 创建）。

### T01 — 初始化工程、依赖与开发启动

**前置：** T00。  
**关联实现：** plan.md 的 T01.01～T01.08。  
**产物核对：** Cargo/npm workspace、锁文件、toolchain、README、.env.example、开发 Compose、根脚本、CI。  
**验证命令/方式：** npm ci；npm run dev:up；npm run check；npm run build；GET /health/live、/health/ready。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T01-BOOT-01 | 干净 checkout，Rust/Node/Docker 可用 | 按 README 安装并启动 | 所有依赖 healthy，页面/API 可达，check/build 退出 0 | Linux 真实8服务 healthy；npm ci、check、unit、build 均0；3页面及代理200 | 通过 | docs/evidence/T01/test-summary.md、integration.txt、build.txt、check.txt |
| T01-BOOT-02 | production 配置样本 | 逐个移除秘密、改 HTTP issuer、改错误 RP_ID | 非零退出，日志只指出配置名，不泄露值 | 有效production配置0，缺秘密/HTTP/RP/弱Cookie等非零；捕获输出不含秘密片段 | 通过 | docs/evidence/T01/integration.txt、non-utf8-regression.txt |
| T01-BOOT-03 | 服务运行 | 停止 Redis 或 PG 后请求 readiness | ready 503，live 200；依赖恢复后 ready 200 | 逐一停止Redis/PG：ready503、live200；恢复后ready200 | 通过 | docs/evidence/T01/integration.txt |

**实现子步骤检查：**

- [x] T01.01：创建第 3.2 节目录及六个 Rust 包、三个前端应用；先仅保留可启动空路由。
- [x] T01.02：固定 Rust 1.98.0 或初始化时经同意的兼容稳定版本；核查库与镜像版本后提交 Cargo.lock/package-lock.json。
- [x] T01.03：配置 cargo fmt、Clippy -D warnings、TypeScript strict、ESLint、Vitest，CI 复用根脚本。
- [x] T01.04：配置 Postgres/Redis/Mailpit 容器健康检查和持久化开发卷；dev:down 默认不删除卷。
- [x] T01.05：实现配置加载与生产安全拒绝启动规则；.env.example 不含真实秘密。
- [x] T01.06：实现 /health/live 无秘密响应；/health/ready 检查 PG 和 Redis，失败返回 503。
- [x] T01.07：创建跨平台脚本封装第 4.4 节命令，依赖缺失返回失败和解决提示；后续尚未创建测试的命令不得返回伪成功。
- [x] T01.08：README 写清 Windows/Linux 前置、生成本地秘密、启动、迁移、前端代理、停止和证据位置。

**验收记录：**

- 代码版本：无Git仓库；docs/evidence/T01/source-sha256.txt。
- 环境与时间：2026-10-07 Linux/WSL2；docs/evidence/T01/environment.md。
- 命令退出码：npm ci/check/unit/tooling/build/dev:up/integration(T01) 均0，verify:docs 0；原始失败见test-summary。
- 失败/阻塞项：已修配置及构建/脚本问题；Windows/远端CI未执行，不宣称通过。
- 修复与复测：docs/evidence/T01/test-summary.md；失败原始输出保留，全套当前验收已通过。
- 任务结论：通过。

### T02 — 威胁模型、状态机与接口契约

**前置：** T01。  
**关联实现：** plan.md 的 T02.01～T02.07。  
**产物核对：** docs/architecture.md、OpenAPI、威胁表、状态机图、ADR：即时撤销/机密客户端/恢复边界。  
**验证命令/方式：** npm run verify:docs；OpenAPI schema lint/parse（由根脚本封装）。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T02-SPEC-01 | 接口草案 | 校验 OpenAPI 并逐路由检查认证与错误 | 第 6.2 节所有操作均覆盖，无未定义成功分支 | 59操作/48JSON完整匹配计划；125 schema及598唯一示例验证；错误/认证/CSRF/分页与负向篡改测试通过 | 通过 | docs/evidence/T02/openapi.txt、tooling.txt、api-coverage.md |
| T02-SPEC-02 | 威胁矩阵 | 检查每种威胁对应测试编号 | 关键威胁均有实现和验收，无用免责声明替代防护 | 34威胁各关联真实任务/案例/E；六状态机与锁序/撤销边界审查；规范与3 ADR已核查 | 通过 | docs/evidence/T02/threat-matrix.txt、spec-sources.md、docs/threat-model.md |

**实现子步骤检查：**

- [x] T02.01：阅读规范并记录来源版本：OIDC Core/Discovery、RFC 7636/7009/7662/9700、RP-Initiated Logout、WebAuthn。
- [x] T02.02：画出信任边界；为枚举、CSRF、XSS、授权截获、重放、并发、代理伪造、依赖故障建立威胁条目。
- [x] T02.03：为账号、挑战、会话、授权码、刷新家族、管理员绑定绘制有限状态机。
- [x] T02.04：按照第 6 节编写每个 JSON API schema、例子、认证/CSRF/限流/错误码；协议端点单独说明。
- [x] T02.05：定义 authenticated/mfa_required/reauth_required 等响应分支与前端转换，不使用任意字符串判状态。
- [x] T02.06：定义安全事件字典、日志禁用字段、request_id、指标低基数标签。
- [x] T02.07：建立威胁→实现任务→验收案例矩阵，明确正在执行的业务请求不在撤销回滚范围内。

**验收记录：**

- 代码版本：无Git仓库；docs/evidence/T02/source-sha256.txt。
- 环境与时间：2026-10-07 Linux，Node22.22.1；规范HTTP200记录见spec-sources。
- 命令退出码：verify:docs、verify:openapi、test:tooling、check均0，tooling 59测试通过。
- 失败/阻塞项：原文方法/路径/hint及工具/schema同步问题已修；不表示协议服务已经实现。
- 修复与复测：docs/evidence/T02/test-summary.md、document-revisions.md。
- 任务结论：通过。

### T03 — 数据库迁移、仓储与事务原语

**前置：** T02。  
**关联实现：** plan.md 的 T03.01～T03.07。  
**产物核对：** 版本化 SQL、仓储接口、数据库测试隔离、可控时钟、清理索引。  
**验证命令/方式：** npm run db:migrate；npm run test:integration -- --task=T03；npm run check。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T03-DB-01 | 空测试库 | 执行迁移两次，再核对索引约束 | 第一次建库，第二次无破坏，约束齐全 | identity_test隔离schema迁移两次、17表及CHECK/UNIQUE/FK/31显式索引齐备；开发库迁移0 | 通过 | docs/evidence/T03/integration.txt、migration.txt |
| T03-DB-02 | 相同规范邮箱 | 并发注册插入两次 | 只存在一行，另一请求被可解释处理 | 同步屏障10路规范邮箱插入仅1成功/1行 | 通过 | docs/evidence/T03/integration.txt |
| T03-DB-03 | 一个未消费 challenge/action | 并发消费十次 | 仅一次成功，其他不得产生会话 | challenge/action各10路仅1消费；登录1session；rollback/半提交/到期拒绝；额外code/family/outbox闭环均通过 | 通过 | docs/evidence/T03/integration.txt |
| T03-DB-04 | 测试脚本 | 把目标切为 production 或非白名单库 | 执行前拒绝，不删除/插入生产数据 | production、非白名单及dbname覆盖均在连接前非零拒绝；仅清理本轮identity_test schema | 通过 | docs/evidence/T03/integration.txt |

**实现子步骤检查：**

- [x] T03.01：创建第 7 节所有实体，先写 CHECK/UNIQUE/FK、时间字段与 token 摘要类型约束。
- [x] T03.02：建立热路径索引，明确禁止用户提供任意排序和 SQL 字段。
- [x] T03.03：为用户锁定、会话检查、challenge 消费、code 消费、token 家族和 outbox 领取写仓储接口。
- [x] T03.04：定义统一锁顺序和事务边界；不在 handler 分散提交同一安全动作。
- [x] T03.05：迁移命令验证目标环境，测试使用独立 identity_test 库；不接受 production seed。
- [x] T03.06：新增可替换时钟用于纯逻辑，数据库边界测试通过明确 timestamps 构造到期。
- [x] T03.07：为迁移升级和事务 rollback 编写真实数据库测试；增加测试结束清理策略，不清理非测试库。

**验收记录：**

- 代码版本：无Git仓库；docs/evidence/T03/source-sha256.txt。
- 环境与时间：2026-10-07 Linux/WSL2；Rust1.98.0 Node22.22.1，实际报告见证据。
- 命令退出码：相关迁移/真实integration或浏览器检查、check/unit/build均0。
- 失败/阻塞项：失败过程已保留并修复；生产/跨平台/完整账号业务未执行。
- 修复与复测：docs/evidence/T03/test-summary.md。
- 任务结论：通过。

### T04 — 安全基础、密码服务、限流与审计

**前置：** T03。  
**关联实现：** plan.md 的 T04.01～T04.08。  
**产物核对：** 密码与加密服务、安全中间件、限流、审计原语及安全单元测试。  
**验证命令/方式：** npm run test:unit；npm run test:integration -- --task=T04；npm run check。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T04-SEC-01 | 密码服务 | 测试长度边界、Unicode、弱密码、错误密码与 hash 参数 | 规则正确，错误密码拒绝，参数符合约定 | 8个新增core安全测试：Unicode/弱清单/64MiBArgon2正确错密/旧PHC/dummy/全进程250ms预算等真实通过 | 通过 | docs/evidence/T04/core-unit.txt |
| T04-SEC-02 | 认证入口 | 缺 CSRF/错误 Origin/伪造代理/超限请求 | 403/429；代理头不改变可信来源；Retry-After 合理 | 真实TCP/PG/Redis验证403、10并发预算、429/Retry-After、伪造XFF无效；Cookie/当前sessionCSRF/邮件预算隔离通过 | 通过 | docs/evidence/T04/integration.txt |
| T04-SEC-03 | Redis 停止 | 请求需限流的登录/挑战接口 | 503，不绕过限流；日志无 secrets | 真实停止Redis/PG：HTTP503无认证旁路，依赖恢复；日志无合成secret片段 | 通过 | docs/evidence/T04/integration.txt |
| T04-SEC-04 | 加密输出 | 篡改 ciphertext、nonce、AAD 或 key | 全部解密失败；相同输入不同 nonce | AEAD user/purpose/kid/nonce/cipher/key篡改均失败，独立nonce及旧key兼容，HMAC已知vector和重复kid拒绝 | 通过 | docs/evidence/T04/core-unit.txt |

**实现子步骤检查：**

- [x] T04.01：实现邮箱规范化与密码规则，加载注明来源的弱密码清单。
- [x] T04.02：实现 Argon2id hash/verify，dummy hash 初始化一次，受控阻塞池、等待上限和参数耗时指标。
- [x] T04.03：实现 256 位随机 token、摘要、常量时间秘密比较和 AES-GCM AAD/版本封装。
- [x] T04.04：建立主/预认证 Cookie、CSRF、Origin、request_id、body limit、response header 中间件。
- [x] T04.05：实现 Redis Lua 原子限流及 Retry-After，使用 HMAC 化账号限流键避免暴露邮箱。
- [x] T04.06：仅信任已配置代理；验证伪造 X-Forwarded-For 无效。
- [x] T04.07：实现低敏日志与审计服务，不输出请求体/query/token；安全变更审计参与事务。
- [x] T04.08：测试密码 Unicode/边界、AEAD 篡改、CSRF、限流竞争和依赖失效。

**验收记录：**

- 代码版本：无Git；docs/evidence/T04/source-sha256.txt。
- 环境与时间：2026-10-07 Linux，Rust1.98.0，真实PG17/Redis7.4测试。
- 命令退出码：core-unit/unit/check/build/testsecurity(T04)/verify/tooling均0，RustSec270依赖0漏洞。
- 失败/阻塞项：接口pattern、缺redis Script feature、Docker data缺失等已修，原失败保留；此关卡仅安全原语不冒充登录业务。
- 修复与复测：docs/evidence/T04/test-summary.md、dependency-source.md。
- 任务结论：通过。

### T05 — 注册、邮箱验证与 outbox 邮件

**前置：** T04。  
**关联实现：** plan.md 的 T05.01～T05.08。  
**产物核对：** 注册/验证 API、邮件模板、Worker、真实 Mailpit 验收。  
**验证命令/方式：** npm run test:integration -- --task=T05；npm run test:e2e -- --task=T05。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T05-MAIL-01 | 新邮箱、Mailpit | 注册，读取邮件，打开再点击确认 | 打开不消费，点击后 verified，重复确认被拒 | 真实注册/加密outbox/SMTP Mailpit送达，GET不消费、按钮POST单消费；verified且无自动session，成功清密文 | 通过 | docs/evidence/T05/integration.txt、e2e.txt |
| T05-MAIL-02 | 一个未用验证链接 | 重发，再使用旧/新链接 | 旧链接失效，新链接只可成功一次 | 旧链接重发后409；新链接一次成功；真实浏览器两case包含重发旧/新链接 | 通过 | docs/evidence/T05/integration.txt、e2e.txt |
| T05-MAIL-03 | SMTP 停止 | 创建注册，确认 outbox，恢复 SMTP | 队列保留并按重试发送；无丢失 | 真实停止Mailpit，pending密文及重试计划保留；恢复并显式构造到期后delivered且清秘密 | 通过 | docs/evidence/T05/integration.txt、e2e.txt |
| T05-MAIL-04 | 已存在与不存在邮箱 | 比较注册/重发状态码及页面 | 统一 202，不含存在信息 | 已有/不存在注册及重发统一202/body，目标预算超额仍202；旧密码及outbox不被重复注册修改 | 通过 | docs/evidence/T05/integration.txt、e2e.txt |

**实现子步骤检查：**

- [x] T05.01：注册统一返回 202，对新邮箱创建用户、验证动作、加密 outbox 参数，同事务提交。
- [x] T05.02：实现重发验证，新动作创建时作废旧动作，账号维度限制不泄露存在性。
- [x] T05.03：邮件链接由固定 ISSUER 生成，token 放 fragment；前端确认按钮 POST，不在 GET 消费。
- [x] T05.04：验证 action 的目的、user、expiry、未消费状态，成功标记 verified 并消费。
- [x] T05.05：Worker 使用 SKIP LOCKED 和租约领取，提交后再调用 SMTP；支持至少一次，重复投递链接幂等安全。
- [x] T05.06：首次失败后按 1/5/15/60/180 分钟重试，下一次失败标为永久失败并指标告警。
- [x] T05.07：SMTP 开发连接 Mailpit；生产验证 TLS/证书，正文包含用途、到期和非本人提示。
- [x] T05.08：成功投递后清除 outbox 中秘密模板参数；日志只保留任务 ID 和结果。

**验收记录：**

- 代码版本：本次Git初始化提交；docs/evidence/T05/source-sha256-final.txt。
- 环境与时间：2026-10-07 Linux/WSL2，Rust1.98.0 Node22.22.1，PG17.11/Redis7.4.11/Mailpit1.31.4/Chromium153。
- 命令退出码：check/unit/build/integration(T05)/e2e(T05)均0；最终Playwright 2 passed、0 failed；工具59测试通过。
- 失败/阻塞项：fetch原生receiver及第二case误查status已修，原失败保留；在线RustSec更新失败，缓存新锁278依赖扫描0。
- 修复与复测：docs/evidence/T05/test-summary.md，最终2case证据e2e.txt。
- 任务结论：通过。

### T06 — 密码登录、账号查询和会话撤销

**前置：** T05。  
**关联实现：** plan.md 的 T06.01～T06.08。  
**产物核对：** 登录、me、会话列表及退出 API。  
**验证命令/方式：** npm run test:integration -- --task=T06；npm run test:e2e -- --task=T06。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T06-SES-01 | 已验证用户 | 密码登录并检查 Cookie/me | 正确身份；HttpOnly、Path、SameSite 等符合环境 | 真实密码登录、me、12h期限及Cookieflags成功；未知/错密/未验证/禁用统一401，MFA只有限挑战 | 通过 | docs/evidence/T06/integration.txt、e2e.txt |
| T06-SES-02 | 预设旧会话 | 登录，尝试复用旧会话 | 新 Cookie 不同，旧流程不可冒用 | Cookie/CSRF轮换；旧预auth无效；main-only重新登录与OAuth事务绑定同提交，MFA新预auth不发普通会话 | 通过 | docs/evidence/T06/integration.txt、e2e.txt |
| T06-SES-03 | 用户两设备及另一用户 | 撤销本人的设备，尝试撤销他人 ID | 目标设备下次拒绝，他人操作 404/403 | 本人两设备及另一用户；跨用户404/403，目标及派生grant/token立即无效，分页/全部/重复退出通过 | 通过 | docs/evidence/T06/integration.txt、e2e.txt |
| T06-SES-04 | 登录与禁用并发 | 用同步屏障交错执行 | 禁用提交后无新有效会话 | 真实HTTP密码POST在用户行锁等待，禁用先提交后不签发session；过期由数据库时间构造 | 通过 | docs/evidence/T06/integration.txt、e2e.txt |

**实现子步骤检查：**

- [x] T06.01：按规范邮箱查询，校验密码，未存在用 dummy hash，统一错误；检查 verified/status。
- [x] T06.02：密码验证后在事务重读账号版本和状态，避免验证期间禁用仍生成会话。
- [x] T06.03：无 TOTP 创建普通会话；有 TOTP 只返回有限 challenge，禁止设置主会话 Cookie。
- [x] T06.04：轮换 Cookie/CSRF，并安全转移当前授权事务到新会话。
- [x] T06.05：实现 me 和分页会话列表，脱敏 UA，只返回本人会话。
- [x] T06.06：实现当前退出、指定设备退出、全部退出；对应 grant 在同事务撤销。
- [x] T06.07：GET 只读，DELETE/POST 必须认证和 CSRF；重复退出幂等。
- [x] T06.08：为 Cookie flags、会话固定、过期、他人会话 ID 和禁用竞争测试。

**验收记录：**

- 代码版本：本次模块Git提交（父提交2fce25e）；source-sha256.txt。
- 环境与时间：2026-10-08 Linux/WSL2；environment.md。
- 命令退出码：check/unit/build/integrationT06/e2eT06/verify/tooling均0；22前端测试、62脚本测试、2E2E通过。
- 失败/阻塞项：接口Option/queryparse、Clippy枚举/审计参数已修；已有远端Windowsunit/T03CI失败原因未取得，不宣称CI全绿。
- 修复与复测：docs/evidence/T06/test-summary.md。
- 任务结论：通过。

### T07 — 找回密码、密码修改与安全通知

**前置：** T06。  
**关联实现：** plan.md 的 T07.01～T07.07。  
**产物核对：** reset/change API、单次重置 action、全部撤销。  
**验证命令/方式：** npm run test:integration -- --task=T07；npm run test:e2e -- --task=T07。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T07-PWD-01 | 已有用户、多会话 | 申请并完成找回，再测试旧密码与会话 | 旧密码/会话失效；新密码登录，无自动会话 | 真实reset邮件/按钮POST，新密码有效、旧密码/全会话与派生token失效，无自动登录，安全通知送达清密文 | 通过 | docs/evidence/T07/integration.txt、e2e.txt |
| T07-PWD-02 | 一个 reset token | 并发两次确认，另用 verify token 重置 | 只有一次成功，跨目的失败 | 10同步HTTP请求仅1成功，verify token不能重置，失败审计事务整体回滚 | 通过 | docs/evidence/T07/integration.txt、e2e.txt |
| T07-PWD-03 | TOTP 用户 | 邮件重置后用新密码登录 | 仍需 TOTP；不能借重置新增 Passkey | 既有TOTP/Passkey未删；新密码仍返回有限MFA挑战；凭邮箱重置不新增登录方式 | 通过 | docs/evidence/T07/integration.txt、e2e.txt |
| T07-PWD-04 | 过期近期认证 | 调用密码修改 | 提示重新认证，不修改密码 | 必须显式当前session近期密码确认；5分钟边界拒绝，已有MFA仅密码仍要求strong | 通过 | docs/evidence/T07/integration.txt、e2e.txt |

**实现子步骤检查：**

- [x] T07.01：找回申请统一 202；存在用户创建 reset action 和通知 outbox，不暴露状态。
- [x] T07.02：重置页面 fragment 读取清除；确认必须 token+新密码，不 GET 自动消费。
- [x] T07.03：事务内锁用户/action，验证用途/expiry，更新 hash/version，consume，撤销全会话/grants。
- [x] T07.04：成功不自动登录，保留 TOTP 和 Passkey，转登录页。
- [x] T07.05：密码修改验证近期认证；无 MFA 密码确认，有 MFA 强认证确认。
- [x] T07.06：密码变更发送通知但 SMTP 故障不回滚完成的安全事务。
- [x] T07.07：测试 action 重放、交叉目的、并发消费、reset 与 refresh 竞争。

**验收记录：**

- 代码版本：模块Git提交（父a4a1764）；source-sha256.txt。
- 环境与时间：2026-10-08 Linux/WSL2，真实PG/Redis/Mailpit/Chromium153。
- 命令退出码：check/unit/build/integrationT07/e2eT07/工具均0；26UI/Vitest、62工具、2E2E通过。
- 失败/阻塞项：首次过严Set-Cookie断言及fixture时间边界已修；真实刷新端点并发T12/T20回归，不冒充现端点。
- 修复与复测：docs/evidence/T07/test-summary.md。
- 任务结论：通过。

### T08 — TOTP、恢复码与近期认证

**前置：** T06、T07。  
**关联实现：** plan.md 的 T08.01～T08.08。  
**产物核对：** TOTP enrollment、challenge verify、恢复码及 reauth API。  
**验证命令/方式：** npm run test:unit；npm run test:integration -- --task=T08；npm run test:e2e -- --task=T08。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T08-MFA-01 | 密码账号 | 开始绑定，先错码后正确码；再次登录 | 错码不启用，正确码启用，登录必须 MFA | 真实AEAD种子绑定错误码不启用/正确码确认十恢复码；密码登录有限挑战，独立RFC验证码后才普通会话 | 通过 | docs/evidence/T08/integration.txt、e2e.txt |
| T08-MFA-02 | 有效六位码 | 并发重复验证 | 仅一次成功；超五次销毁挑战 | 同时间步十并发最多1成功，freshchallenge重放拒绝；5错持久作废，合法码不能复活 | 通过 | docs/evidence/T08/integration.txt、e2e.txt |
| T08-MFA-03 | 恢复码集合 | 并发使用一个码，再重新生成 | 仅一次成功，旧集合全部失效 | 同恢复码10并发一次会话；重建旧集合全部无效，数据库只存摘要 | 通过 | docs/evidence/T08/integration.txt、e2e.txt |
| T08-MFA-04 | TOTP 用户仅密码近期认证 | 关闭 TOTP/重建恢复码 | 拒绝并要求强认证 | 先让最近强认证窗口真实过期，再密码确认不能关闭/重建；真实TOTP strong proof后可重建 | 通过 | docs/evidence/T08/integration.txt、e2e.txt |

**实现子步骤检查：**

- [x] T08.01：实现有限重新认证流程；无 MFA 密码可确认首次绑定，已有 MFA 必须第二因素或 UV。
- [x] T08.02：生成 160 位以上 TOTP 种子，AES-GCM 绑定 user/purpose，未确认状态限时保留。
- [x] T08.03：确认有效验证码后原子开启因子，并生成十个单次恢复码；只返回一次原文。
- [x] T08.04：登录 challenge 绑定预认证/用户/用途，最多五次，成功消费并创建会话。
- [x] T08.05：数据库原子 last_step 校验，拒绝同时间步重放，允许约定容差。
- [x] T08.06：恢复码 atomically consume，成功可完成登录/近期认证，不可反复复用。
- [x] T08.07：重建恢复码覆盖旧码；关闭 TOTP 要近期强认证，记录审计和通知。
- [x] T08.08：不得提供管理员无条件重置 MFA 路由。

**验收记录：**

- 代码版本：当前模块Git提交（父a4320d2）；source-sha256.txt。
- 环境与时间：2026-10-08 Linux/PG17/Redis7.4/Chromium153；受控Clock重放+实际SystemClock算法验证。
- 命令退出码：check/unit/build/integrationT08/e2eT08/工具均0；29前端测试、RFC算法2测试、1完整E2E通过。
- 失败/阻塞项：测试强窗口及Redis真实限流干扰已修场景隔离；不改生产预算或last_step；失败保留。
- 修复与复测：docs/evidence/T08/test-summary.md。
- 任务结论：通过。

### T09 — Passkey 注册、登录和管理

**前置：** T06、T08。  
**关联实现：** plan.md 的 T09.01～T09.08。  
**产物核对：** WebAuthn API、凭证管理、虚拟与真实设备验收。  
**验证命令/方式：** npm run test:integration -- --task=T09；npm run test:e2e -- --task=T09；真实设备手工验收。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T09-PK-01 | 可注册账号、虚拟认证器 | 注册、退出、discoverable 登录 | 成功识别本人并达到强认证 | CDP虚拟CTAP2生成真实凭证/签名；注册、discoverable登录及Passkey强reauth成功 | 通过 | docs/evidence/T09/e2e.txt |
| T09-PK-02 | 已有 challenge | 错误 origin/无 UV/重放 assertion | 全部失败，无会话 | fresh未消费签名：错origin/challenge/signature各拒；原合法签名成功；重放拒；重新有效签名UVfalse拒绝 | 通过 | docs/evidence/T09/e2e.txt及测试源码 |
| T09-PK-03 | 两个用户 | 跨账号删除 credential、超过十个 | 越权拒绝，数量限制有效 | PG真实近期认证保护、加密挑战、跨user删除404、最多10凭证限制；HMAC分页及last_used实际 | 通过 | docs/evidence/T09/integration.txt |
| T09-PK-04 | 真实手机/桌面认证器 | 注册、登录、取消、备选密码 | 真实流程成功，取消可恢复 | 当前只有虚拟认证器，外部手机/桌面设备及Safari尚未提供 | 阻塞 | docs/evidence/T09/test-summary.md |

**实现子步骤检查：**

- [x] T09.01：构建 webauthn-rs RP，固定 origin/RP_ID，要求 UV 和 discoverable credential。
- [x] T09.02：注册先核验近期认证，保存一次性加密状态及 user/session/purpose 绑定。
- [x] T09.03：验证完成使用库检查 challenge/origin/RP/signature/UV 后保存凭证；credential ID 唯一。
- [x] T09.04：登录 options 不泄露账号，discoverable assertion 后由已存凭证识别用户。
- [x] T09.05：credential 状态更新、challenge 消费及会话生成同事务；验证前后检查用户 enabled。
- [x] T09.06：支持 Passkey reauth、名称编辑与删除，最多十个。
- [x] T09.07：按库处理同步凭证计数器，不自行要求所有 signCount 递增。
- [ ] T09.08：Playwright 虚拟认证器覆盖各负向场景；真实设备验收不可用则保持待验收。

**验收记录：**

- 代码版本：当前模块Git提交，父6f39d22。
- 环境与时间：2026-10-08 Linux/PG17/Redis7.4/Chromium153 CDP；真实设备不可用。
- 命令退出码：check/unit/build/APIintegration及2E2E均0；真实设备case未运行。
- 失败/阻塞项：T09-PK-04真实设备待验收；标准clientDataJSON与库uvm扩展已修，测试失败保留。
- 修复与复测：docs/evidence/T09/test-summary.md、contract-revisions.md。
- 任务结论：待验收。

### T10 — 受管理客户端与授权/同意事务

**前置：** T06、T02。  
**关联实现：** plan.md 的 T10.01～T10.08。  
**产物核对：** 客户端仓储、authorize、transaction view/decision、同意记录。  
**验证命令/方式：** npm run test:integration -- --task=T10；npm run test:e2e -- --task=T10。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T10-AUTHZ-01 | 注册客户端 | GET/POST form 合法请求、首次同意、拒绝同意 | 正确流程；拒绝返回 access_denied+state | 真实GET/POSTform授权→登录续接→同意生成60秒code；拒绝回access_denied/state，只有注册回调 | 通过 | docs/evidence/T10/integration.txt、e2e.txt |
| T10-AUTHZ-02 | 错误/前缀/通配符回调 | 请求 authorize | 留在身份域，不开放重定向 | 无效/前缀/通配回调不外跳；重复参数/PKCE降级/未知return_to/非法state、nonce、scope拒绝 | 通过 | docs/evidence/T10/integration.txt、e2e.txt |
| T10-AUTHZ-03 | 未登录/无同意 | prompt=none 请求 | 规范 login_required/consent_required | 未登录promptnone login_required、未同意consent_required且state正确；promptlogin需新session，maxage旧认证拒绝 | 通过 | docs/evidence/T10/integration.txt、e2e.txt |
| T10-AUTHZ-04 | 两个浏览器事务 | 交换事务 ID 或重复 decision | 拒绝跨浏览器/重复操作 | 浏览器事务交换拒；十并发decision一次消费；client禁用拒发code，扩scope须同意 | 通过 | docs/evidence/T10/integration.txt、e2e.txt |

**实现子步骤检查：**

- [x] T10.01：实现客户端 service；后台在 T14 暴露，当前测试 fixture 注册 A/B。
- [x] T10.02：生成随机 client secret，只存摘要，回调严格验证并区分 login/logout。
- [x] T10.03：authorize 严格解析单值参数，检查 client/redirect/scopes/PKCE/state/nonce/prompt/max_age。
- [x] T10.04：验证后保存授权事务并绑定当前浏览器；无效回调不向外部跳转。
- [x] T10.05：处理未登录、prompt=login、max_age、prompt=none 和已有同意。
- [x] T10.06：登录成功恢复原事务，不让浏览器改 client/redirect/scope。
- [x] T10.07：同意页读取服务端已验证参数；POST 同意/拒绝有 CSRF，决定原子单次消费。
- [x] T10.08：签发 grant/code 的逻辑连接 T11，不提供跳过同意的临时 endpoint。

**验收记录：**

- 代码版本：当前模块Git提交（父d8a1434）。
- 环境与时间：2026-10-08 Linux/PG17/Redis7.4/Chromium153，同源协议代理。
- 命令退出码：check/unit/build/APIintegration及2E2E均0，36前端/62工具测试通过。
- 失败/阻塞项：非法PKCE示例、开发代理同源、未知return_to已修，失败证据保留；签名/交换在T11。
- 修复与复测：docs/evidence/T10/test-summary.md、contract-revisions.md。
- 任务结论：通过。

### T11 — 授权码交换、ID Token、Discovery 和 Userinfo

**前置：** T10。  
**关联实现：** plan.md 的 T11.01～T11.08。  
**产物核对：** token/code grant、JWKS、discovery、userinfo、签名密钥兼容。  
**验证命令/方式：** npm run test:integration -- --task=T11；npm run test:security。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T11-OIDC-01 | 授权成功 code | 成熟客户端交换并验签 | iss/aud/nonce/exp/signature 符合规范 | openid-client6.8.8真实discovery/Basic/PKCE/nonce兑换，独立jose6.2.12验签与算法/iss/aud/expiry负向通过 | 通过 | [docs/evidence/T11/integration.txt](docs/evidence/T11/integration.txt) |
| T11-OIDC-02 | 同一 code | 错误 verifier/client/redirect 和并发重复消费 | 非法失败，合法最多一次成功 | 错误verifier/client/redirect拒；十并发仅一成功；签名失败不消费code | 通过 | [docs/evidence/T11/integration.txt](docs/evidence/T11/integration.txt) |
| T11-OIDC-03 | 不同 scope token | 访问 userinfo | sub 稳定，email/profile 不越权返回 | scope字段对照、稳定sub；refresh不能访问userinfo；禁用当前权限立即拒 | 通过 | [docs/evidence/T11/integration.txt](docs/evidence/T11/integration.txt) |
| T11-OIDC-04 | JWKS | 检查公私钥和 discovery 能力 | 无私钥字段，声明与实际一致 | 公开仅RSA公钥；固定RS256/kid；当前仅公布真实authorization_code | 通过 | [docs/evidence/T11/integration.txt](docs/evidence/T11/integration.txt) |

**实现子步骤检查：**

- [x] T11.01：实现 code 原子消费，绑定 grant/client/redirect/PKCE；固定客户端 Basic 验证。
- [x] T11.02：创建不透明 access/refresh token 摘要及 family，expiry 不超过会话。
- [x] T11.03：RS256 签发 ID Token，固定 iss/aud/sub/nonce/auth_time/amr/sid；禁止算法协商降级。
- [x] T11.04：实现 discovery，只声明实际功能；JWKS 仅公钥，当前 kid 唯一。
- [x] T11.05：实现 userinfo 每次检查有效状态并按 scope 限制字段。
- [x] T11.06：token/认证响应 no-store，日志不打印 body/token。
- [x] T11.07：支持当前与旧公钥窗口，明确正常 token exp 与退出 hint 的差别。
- [x] T11.08：用成熟外部 OIDC client 互操作测试，不只用自写解析器自证。

**验收记录：**

- 代码版本：本模块 Git 提交（父提交9d76a11）。
- 环境与时间：2026-10-08 Linux、Rust1.98/Node22、真实PG17/Redis7.4；openid-client6.8.8/jose6.2.12。
- 命令退出码：final-check/unit/build/docs/openapi/tooling/integration均0；63工具测试、40前端测试及Rust单元通过。
- 失败/阻塞项：初始fmt/needless_borrow已修并保留失败证据；CI既有Windows/T03失败仍未解决，完整安全验收在T20。
- 修复与复测：[T11结果](docs/evidence/T11/test-summary.md)、[签名契约](docs/oidc-signing.md)。
- 任务结论：通过（当前真实本地模块验收，未宣称OpenID认证）。


### T12 — 刷新轮换、Introspection、撤销与 RP 退出

**前置：** T11。  
**关联实现：** plan.md 的 T12.01～T12.08。  
**产物核对：** refresh/introspect/revoke/logout；即时撤销并发测试。  
**验证命令/方式：** npm run test:integration -- --task=T12；npm run test:security。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T12-REV-01 | 有效 grant/token | 退出事务提交后立即检查 | active=false，无正缓存延迟 | 待填写 | 未执行 | 待提供：时间线与测试 |
| T12-REV-02 | 已轮换 refresh | 再次提交旧 refresh | 整个 family 无效，审计保留 | 待填写 | 未执行 | 待提供：重放输出 |
| T12-REV-03 | 客户端 B、A token | B introspect/revoke A token | 不暴露、不撤销 A 授权 | 待填写 | 未执行 | 待提供：隔离测试 |
| T12-REV-04 | RP 退出请求 | GET/POST form入口、非法回调、合法确认 POST | GET/POST form入口不撤销；非法不跳转；确认后撤销 | 待填写 | 未执行 | 待提供：跳转和状态证据 |
| T12-REV-05 | 退出和刷新同步屏障 | 并发交错执行 | 撤销提交后没有新有效凭证 | 待填写 | 未执行 | 待提供：并发输出 |

**实现子步骤检查：**

- [ ] T12.01：按统一锁顺序验证 refresh 所属用户/会话/grant/client，消费旧 token 后轮换。
- [ ] T12.02：重放旧 refresh 时提交家族撤销及审计，然后返回标准错误；不能错误 rollback 撤销。
- [ ] T12.03：introspection 做 Basic 验证并核对归属，直接查询权威状态；unknown/inactive 返回 active=false。
- [ ] T12.04：revoke 按第 6.1 节行为实现幂等，禁止其他客户端撤销。
- [ ] T12.05：完善当前/全部/单应用撤销，使既有 code 和 tokens 一并失效。
- [ ] T12.06：RP logout GET 校验 hint/回调/state，显示确认；POST 检查 CSRF 后撤销当前 sid。
- [ ] T12.07：检查刷新/退出、登录/禁用、introspection/撤销竞争，精确定义事务提交边界。
- [ ] T12.08：测试 Postgres/Redis 不可用时错误，不返回伪 active。

**验收记录：**

- 代码版本：待填写。
- 环境与时间：待填写。
- 命令退出码：待填写。
- 失败/阻塞项：待填写。
- 修复与复测：待填写。
- 任务结论：未开始。

### T13 — 两个 BFF 演示应用与接入指南

**前置：** T12。  
**关联实现：** plan.md 的 T13.01～T13.08。  
**产物核对：** demo A/B、BFF 服务端会话、开发客户端种子、接入文档。  
**验证命令/方式：** npm run test:e2e -- --task=T13；npm run test:integration -- --task=T13。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T13-BFF-01 | A/B 两应用 | A 登录，进入 B，再查看两应用 | B 无需输入凭证，首次同意仍出现 | 待填写 | 未执行 | 待提供：视频/截图与 trace |
| T13-BFF-02 | 两应用已登录 | IdP 退出全部设备后访问 A/B | 下一次保护请求均拒绝 | 待填写 | 未执行 | 待提供：E2E 输出 |
| T13-BFF-03 | 浏览器和服务日志 | 检查存储、网络、日志 | 无 OAuth tokens/client secret 暴露 | 待填写 | 未执行 | 待提供：脱敏检查 |
| T13-BFF-04 | 多个并发请求需刷新 | 并发访问，再中断 introspection | 只一次轮换；故障 503，无旁路 | 待填写 | 未执行 | 待提供：并发测试 |

**实现子步骤检查：**

- [ ] T13.01：同一 BFF 包运行 A/B 独立实例，不同 client secret、回调、Cookie 与会话空间。
- [ ] T13.02：使用维护中的 OIDC 客户端库处理 discovery、state、nonce、PKCE 与 ID Token；BFF session 持久化。
- [ ] T13.03：服务端存储 OAuth token，BFF 数据库中的长期秘密加密，不回传网页。
- [ ] T13.04：每次受保护 API introspection，无效清 session；状态服务不可用返回 503，不当作已登录。
- [ ] T13.05：刷新通过共享会话锁串行化，失败清理，不无限重试旧 token。
- [ ] T13.06：提供本应用退出与身份平台退出两个明确按钮；本地退出不谎称退出其他设备。
- [ ] T13.07：验证 A 登录后 B 复用 IdP session，但首次 B scope 仍按同意规则。
- [ ] T13.08：写完整接入指南、示例环境和撤销请求失败处理。

**验收记录：**

- 代码版本：待填写。
- 环境与时间：待填写。
- 命令退出码：待填写。
- 失败/阻塞项：待填写。
- 修复与复测：待填写。
- 任务结论：未开始。

### T14 — 管理员初始化与管理 API

**前置：** T08、T10、T12。  
**关联实现：** plan.md 的 T14.01～T14.08。  
**产物核对：** admin CLI、受限初始化、用户/客户端/管理员/审计 API。  
**验证命令/方式：** npm run test:integration -- --task=T14；CLI 初始化手工测试。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T14-ADM-01 | 无管理员库 | 并发初始化，再次初始化 | 只有首个成功，后续拒绝；秘密不回显 | 待填写 | 未执行 | 待提供：CLI 脱敏输出 |
| T14-ADM-02 | 未绑定/普通/合格管理员 | 调用所有后台操作 | 未绑定受限，普通 403，合格允许 | 待填写 | 未执行 | 待提供：权限矩阵测试 |
| T14-ADM-03 | 仅一名管理员 | 删除/禁用本人 | 保护最后管理员 | 待填写 | 未执行 | 待提供：API 测试 |
| T14-ADM-04 | 活动用户与客户端 | 禁用或停用后检查 token；模拟审计写失败 | 凭证无效；审计失败变更 rollback | 待填写 | 未执行 | 待提供：事务测试 |

**实现子步骤检查：**

- [ ] T14.01：CLI 仅数据库无管理员时创建/授予首个管理员，事务防并发重复初始化。
- [ ] T14.02：密码由隐藏交互或 stdin 读取，不放命令行、history、日志。
- [ ] T14.03：未配置因素的首个管理员只有绑定权限，后台必须近期强认证。
- [ ] T14.04：后续授予仅针对已 verified 且有因素的已有用户；拒绝删除/停用最后可用管理员。
- [ ] T14.05：实现分页用户查询、禁用/启用、全会话撤销。
- [ ] T14.06：实现 client 创建/修改/停用/轮换，秘密一次展示；停用同时使当前凭证检查无效。
- [ ] T14.07：实现管理员成员和审计分页；每个 handler 独立校验身份/权限，不依赖 UI 隐藏。
- [ ] T14.08：高风险操作审计与数据库变更同事务，使用明确目标、结果、request_id。

**验收记录：**

- 代码版本：待填写。
- 环境与时间：待填写。
- 命令退出码：待填写。
- 失败/阻塞项：待填写。
- 修复与复测：待填写。
- 任务结论：未开始。

### T15 — Awwwards 研究、设计稿与视觉规范

**前置：** T00；可与后端并行。  
**关联实现：** plan.md 的 T15.01～T15.08。  
**产物核对：** reference-study.md、三页面桌面/手机稿、组件状态与设计 tokens。  
**验证命令/方式：** 本任务以来源核查、设计稿人工验收和对比度检查为主。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T15-DES-01 | 可访问 Awwwards | 逐条核查三案例 | 官方奖项/年份/URL 匹配，非伪造参考 | 官方核实4个2026 Site of the Day案例并保存双尺寸截图，官网失败如实记录 | 通过 | docs/design/reference-study.md、docs/evidence/T15/browser-capture.json |
| T15-DES-02 | 三个页面两尺寸稿 | 评审布局、字体、留白和流程 | 风格一致，品牌与认证目标兼容 | 用户明确回复“采用此方案，进入 T16”，三主页面双尺寸方案通过视觉评审 | 通过 | docs/evidence/T15/user-review.md |
| T15-DES-03 | tokens 和状态稿 | 检查对比度、键盘设计和动效替代 | 满足 AA，状态完整，不依赖动画完成操作 | 对比度/16布局及设计标签、键盘顺序、44px目标、reduced-motion实测通过；完整应用可访问性仍T16～T19验收 | 通过 | docs/evidence/T15/keyboard-review.json、test-summary.md |

**实现子步骤检查：**

- [x] T15.01：访问 Awwwards 官方网站，核实至少三个获奖案例及准确奖项/年份。
- [x] T15.02：保存官方链接、访问日期、截图和研究备注；失败记阻塞，不凭记忆填写获奖。
- [x] T15.03：比较 typography/grid/spacing/color/motion/mobile/performance，提炼各一至两条可迁移原则。
- [x] T15.04：采用第 8.2 节默认 tokens，实际测试对比度；调整必须记录理由。
- [x] T15.05：制作品牌首页、登录页、账号安全中心桌面/手机稿，强调可信和清晰任务路径。
- [x] T15.06：补齐按钮、输入、错误、对话框、loading/empty/unavailable 和 reduced-motion 状态。
- [x] T15.07：由用户按本任务验收设计稿；仅该视觉稿需要设计确认，后端独立继续。
- [x] T15.08：冻结视觉规范后再做 T16，不以获奖参考为理由引入重型动画。

**验收记录：**

- 代码版本：docs/evidence/T15/artifacts-sha256.txt；用户评审记录保留。
- 环境与时间：2026-10-07 Linux/Chromium145；完整访问时间见研究证据。
- 命令退出码：来源/对比度/布局/设计键盘检查0；用户选择采用方案。
- 失败/阻塞项：参考官网浏览器不支持及脚本错误按边界记录，不将其内部流程伪通过。
- 修复与复测：安全页窄屏按钮、对比度控件边框已修复并重测。
- 任务结论：通过。

### T16 — 前端基础、组件和可访问路由

**前置：** T15、T02。  
**关联实现：** plan.md 的 T16.01～T16.08。  
**产物核对：** 设计系统组件、布局、API client、路由和前端测试。  
**验证命令/方式：** npm run check；npm run test:unit；npm run test:accessibility；npm run build。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T16-UI-01 | 组件示例页 | 键盘使用表单/对话框/分页 | 焦点正确，labels 可读，错误明确 | Chromium153实际axe无违规；label/错误/密码切换、Dialog取消/焦点圈定/Escape返回、分页均通过 | 通过 | docs/evidence/T16/browser-check.json、ui-summary.md |
| T16-UI-02 | 四种宽度 | 检查溢出/按钮触达/放大 | 无横向溢出，200% zoom 可用 | 360/390/768/1440无横溢出，控件>=44px；200%文字放大无横溢出，减少动态已启用 | 通过 | docs/evidence/T16/browser-check.json及截图 |
| T16-UI-03 | 网络失败与 CSRF 轮换 | 调用 API 并模拟失败 | 错误可恢复，重新获取 Token，不无限重试 | 6API行为测试含CSRF获取/轮换/失效、失败不自动重放、429信息、取消和网络错误；11UI测试通过 | 通过 | docs/evidence/T16/ui-summary.md、unit-all.txt |

**实现子步骤检查：**

- [x] T16.01：实现 CSS tokens、响应式网格、字体 fallback 和基础 layout。
- [x] T16.02：实现 Button/Input/Password/FieldError/Status/Dialog/Table/CursorPagination/Empty 组件。
- [x] T16.03：实现 CSRF 获取/轮换、统一错误 code、request_id、网络取消及 Query 缓存策略。
- [x] T16.04：创建公开/账号/管理员路由和认证 guard；guard 只是体验，安全以 API 为准。
- [x] T16.05：按路由拆包，后台和二维码/认证器辅助库懒加载。
- [x] T16.06：实现页面标题、焦点恢复、键盘 Tab 顺序、aria-live 和 autocomplete。
- [x] T16.07：组件覆盖 idle/loading/error/disabled，不编写仅检查 className 的镜像测试。
- [x] T16.08：在 360/390/768/1440 px 检查控件和布局。

**验收记录：**

- 代码版本：无Git仓库；docs/evidence/T16/source-sha256.txt。
- 环境与时间：2026-10-07 Linux/WSL2；Rust1.98.0 Node22.22.1，实际报告见证据。
- 命令退出码：相关迁移/真实integration或浏览器检查、check/unit/build均0。
- 失败/阻塞项：失败过程已保留并修复；生产/跨平台/完整账号业务未执行。
- 修复与复测：docs/evidence/T16/test-summary.md。
- 任务结论：通过。

### T17 — 品牌、注册登录、MFA 与密码恢复页面

**前置：** T05～T09、T16。  
**关联实现：** plan.md 的 T17.01～T17.08。  
**产物核对：** 完整公开页面、邮件确认页、认证状态流程。  
**验证命令/方式：** npm run test:e2e -- --task=T17；npm run test:accessibility。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T17-PAGE-01 | 全新用户 | 从首页完成注册、验证、登录 | 所有步骤真实调用，无占位成功 | 待填写 | 未执行 | 待提供：Playwright trace |
| T17-PAGE-02 | MFA 用户 | 密码→TOTP/恢复码，错误/过期后重试 | 不绕过 MFA，状态清晰 | 待填写 | 未执行 | 待提供：E2E 输出 |
| T17-PAGE-03 | 弱网络、浏览器返回 | 找回/重置、取消 Passkey、网络失败 | 可恢复，无秘密持久化或误授权 | 待填写 | 未执行 | 待提供：状态测试与截图 |

**实现子步骤检查：**

- [ ] T17.01：实现首页、注册、邮箱确认、密码登录、Passkey 入口、MFA、找回及重置页面。
- [ ] T17.02：根据 server 状态切换 authenticated/mfa_required，保留已验证 OAuth 事务 ID。
- [ ] T17.03：邮件 fragment 读取清除后保存在短期内存；明确确认按钮，不自动消费。
- [ ] T17.04：MFA 允许粘贴完整验证码及恢复码切换；密码管理器可填充。
- [ ] T17.05：统一邮箱申请成功文案；限流倒计时依据 Retry-After。
- [ ] T17.06：网络失败保留邮箱等非秘密字段；不持久化密码/验证码/token。
- [ ] T17.07：取消 Passkey 有密码回退；过期挑战引导重启而非静默失败。
- [ ] T17.08：处理浏览器返回/刷新/重复提交，实际服务端状态为准。

**验收记录：**

- 代码版本：待填写。
- 环境与时间：待填写。
- 命令退出码：待填写。
- 失败/阻塞项：待填写。
- 修复与复测：待填写。
- 任务结论：未开始。

### T18 — 账号安全、会话、授权及同意界面

**前置：** T12、T16、T17。  
**关联实现：** plan.md 的 T18.01～T18.08。  
**产物核对：** 账号中心、安全设置、会话页、应用授权页和 OIDC 同意页。  
**验证命令/方式：** npm run test:e2e -- --task=T18；npm run test:accessibility。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T18-ACC-01 | 两个设备账号 | 页面撤销另一设备和应用授权 | 列表更新，另一设备/应用下次拒绝 | 待填写 | 未执行 | 待提供：多会话 E2E |
| T18-ACC-02 | 近期认证过期 | 删除因素/修改密码 | 要求合格重新认证，不能 UI 绕过 | 待填写 | 未执行 | 待提供：E2E |
| T18-ACC-03 | 第一次授权 | 检查 scope，拒绝后重新允许 | 拒绝无 code，同意产生正确 grant | 待填写 | 未执行 | 待提供：协议页面记录 |

**实现子步骤检查：**

- [ ] T18.01：账号页显示 verified/email/display_name（若无设置只读）；不出现修改邮箱入口。
- [ ] T18.02：安全页接入密码修改、TOTP、恢复码、Passkey，统一近期认证 dialog。
- [ ] T18.03：恢复码只在生成成功后显示一次，提供复制/下载并说明保存。
- [ ] T18.04：会话页标记当前设备，支持单设备/全部撤销，成功后刷新 Query。
- [ ] T18.05：应用页显示客户端与 scope，撤销 grant 后清理缓存。
- [ ] T18.06：同意页仅显示 transaction 服务端内容，明确允许/拒绝，不接受 URL 修改 scope。
- [ ] T18.07：删除因素、重建码等按钮有明确影响提示，失败保持原状态。
- [ ] T18.08：空/加载/错误/依赖不可用状态有重试路径，不自动重复高风险 POST。

**验收记录：**

- 代码版本：待填写。
- 环境与时间：待填写。
- 命令退出码：待填写。
- 失败/阻塞项：待填写。
- 修复与复测：待填写。
- 任务结论：未开始。

### T19 — 管理后台 UI

**前置：** T14、T16。  
**关联实现：** plan.md 的 T19.01～T19.08。  
**产物核对：** 用户、客户端、管理员、审计页面及操作确认。  
**验证命令/方式：** npm run test:e2e -- --task=T19；npm run test:accessibility。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T19-ADMINUI-01 | 合格管理员 | 创建客户端→轮换→关闭结果 | 功能完整，秘密仅一次显示 | 待填写 | 未执行 | 待提供：截图/trace |
| T19-ADMINUI-02 | 普通用户 | 手工后台路由/请求 | 无数据，后端 403 | 待填写 | 未执行 | 待提供：权限 E2E |
| T19-ADMINUI-03 | 恶意名称、十万用户 | 列表/详情/审计分页 | 文本转义、按页查询，不卡死 | 待填写 | 未执行 | 待提供：XSS/分页输出 |

**实现子步骤检查：**

- [ ] T19.01：实现后台分页列表/过滤、用户详情、禁用启用及会话撤销。
- [ ] T19.02：实现客户端创建、精确回调编辑、停用及秘密轮换。
- [ ] T19.03：秘密只在创建/轮换结果 dialog 显示一次，关闭后不从 API 再取。
- [ ] T19.04：实现管理员成员管理及最后管理员错误提示。
- [ ] T19.05：审计支持固定时间范围及 cursor，不下载全部历史。
- [ ] T19.06：高风险操作先说明影响，再强认证与确认；提交中防重复。
- [ ] T19.07：用户提供名称按文本渲染，禁止任意 HTML。
- [ ] T19.08：普通用户手工打开后台时无数据；后端 403 单独处理。

**验收记录：**

- 代码版本：待填写。
- 环境与时间：待填写。
- 命令退出码：待填写。
- 失败/阻塞项：待填写。
- 修复与复测：待填写。
- 任务结论：未开始。

### T20 — 安全、协议、并发和故障全面验收

**前置：** T13、T18、T19。  
**关联实现：** plan.md 的 T20.01～T20.08。  
**产物核对：** 安全矩阵、依赖/秘密/ZAP 报告、互操作和并发证据。  
**验证命令/方式：** npm run test:security；npm run test:integration；npm run test:e2e；扫描脚本。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T20-AUDIT-01 | 全部服务 | 运行威胁矩阵与安全测试 | 关键场景全过，确认 High/Critical 为零 | 待填写 | 未执行 | 待提供：报告及分诊 |
| T20-AUDIT-02 | 真实依赖 | 停止并恢复各依赖 | 无认证旁路，邮件不丢 | 待填写 | 未执行 | 待提供：故障时间线 |
| T20-AUDIT-03 | 独立客户端 | 完成协议验证和非法 token 测试 | 互操作通过，非法声明拒绝 | 待填写 | 未执行 | 待提供：外部客户端输出 |

**实现子步骤检查：**

- [ ] T20.01：执行依赖漏洞与许可证扫描、秘密扫描，分诊真实风险和误报。
- [ ] T20.02：按威胁表验证 CSRF/XSS/枚举/固定会话/越权/开放重定向/代理伪造。
- [ ] T20.03：检查 PKCE/state/nonce/client/redirect 绑定与 scope、算法混淆、ID Token 错误 audience。
- [ ] T20.04：用同步屏障测试 code、refresh、恢复码双消费、refresh/logout、login/disable。
- [ ] T20.05：真实停止 PG/Redis/SMTP，验证失败关闭或 outbox 重试。
- [ ] T20.06：ZAP 对测试站点 authenticated/unauthenticated 扫描，禁止攻击生产。
- [ ] T20.07：运行维护中 OIDC client 互操作；能运行 OpenID conformance suite 则记录实际模块，未认证不得宣称认证。
- [ ] T20.08：高危/严重问题全部修复再复测；残留低/中风险明确影响和是否阻塞。

**验收记录：**

- 代码版本：待填写。
- 环境与时间：待填写。
- 命令退出码：待填写。
- 失败/阻塞项：待填写。
- 修复与复测：待填写。
- 任务结论：未开始。

### T21 — 性能、容量和交互优化

**前置：** T20。  
**关联实现：** plan.md 的 T21.01～T21.09。  
**产物核对：** 可重放 k6 脚本、性能原始报告、前端体积和体验报告。  
**验证命令/方式：** npm run seed:acceptance；npm run test:load -- --scenario=introspection|password|mixed；npm run build；前端性能脚本。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T21-PERF-01 | 规定硬件与数据 | 单项/综合各 15 分钟 | 达到第 8.4 节门槛，错误分类明确 | 待填写 | 未执行 | 待提供：k6 JSON/summary 与资源图 |
| T21-PERF-02 | 移动模拟环境 | 五次冷加载和关键交互 | LCP/CLS/体积达标，报告方法完整 | 待填写 | 未执行 | 待提供：性能报告 |
| T21-PERF-03 | 查询计划与队列 | 分析慢 SQL、峰值 hash 队列 | 无明显 N+1，资源限制有效 | 待填写 | 未执行 | 待提供：SQL 计划和队列指标 |

**实现子步骤检查：**

- [ ] T21.01：准备第 8.4 节验收环境与种子，拒绝生产运行 seed。
- [ ] T21.02：记录 Argon2 参数、哈希内存/耗时、并行队列；不降安全参数换吞吐。
- [ ] T21.03：每场景两分钟预热，十五分钟测量，单项与综合分开。
- [ ] T21.04：状态检查使用足量真实 token 池及受控更新，避免到期造成假错误；客户端认证包含在耗时内。
- [ ] T21.05：数据库 account 查询跑真实 10 万行；用 EXPLAIN ANALYZE 检查热 SQL。
- [ ] T21.06：监测 API/PG/Redis CPU/内存/连接池/排队，定位瓶颈后只优化相关路径。
- [ ] T21.07：按固定移动设备网络跑至少五次前端测量，报告体积、LCP/CLS 与交互。
- [ ] T21.08：保留失败原始报告，修复后重测；不改阈值掩盖失败。
- [ ] T21.09：记录容量边界及何时拆分数据库/增加 API/降低外部流量，禁止凭空保证无限扩容。

**验收记录：**

- 代码版本：待填写。
- 环境与时间：待填写。
- 命令退出码：待填写。
- 失败/阻塞项：待填写。
- 修复与复测：待填写。
- 任务结论：未开始。

### T22 — 单机生产、监控、备份和密钥轮换

**前置：** T20、T21。  
**关联实现：** plan.md 的 T22.01～T22.11。  
**产物核对：** 生产镜像/Compose/Caddy、部署回滚与恢复 runbook、实际演练。  
**验证命令/方式：** 生产配置验证；npm run build；部署冒烟；备份恢复与轮换 runbook 实际执行。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T22-OPS-01 | 干净 Linux 主机/域名/秘密 | 按 runbook 部署、检查开放端口 | HTTPS、安全 headers、健康正常，DB/Redis 不外露 | 待填写 | 未执行 | 待提供：部署输出及端口检查 |
| T22-OPS-02 | 独立备份与新主机 | 恢复到已知时间并操作账号 | RPO/RTO 达标，密钥可用，日志脱敏 | 待填写 | 未执行 | 待提供：恢复计时和业务检查 |
| T22-OPS-03 | 活动用户与旧 token | 签名/加密密钥轮换 | 新凭证正常，旧窗口兼容，TOTP 不丢 | 待填写 | 未执行 | 待提供：轮换报告 |
| T22-OPS-04 | 邮件域名与监控 | 发真实邮件，制造可控失败告警 | 送达/DNS验证、告警可达 | 待填写 | 未执行 | 待提供：邮件和告警记录 |

**实现子步骤检查：**

- [ ] T22.01：多阶段 Rust/frontend 镜像、非 root、read-only 可用路径、资源上限、明确健康检查和 graceful shutdown。
- [ ] T22.02：生产仅开放 80/443（80 仅跳转/证书），SSH 受控；DB/Redis 不公开。
- [ ] T22.03：秘密文件受权限控制挂载；固定 issuer/RP；配置 HSTS/no-store/CSP。
- [ ] T22.04：部署流程：配置验证→备份成功→迁移→启动→readiness→冒烟；迁移失败停止发布。
- [ ] T22.05：保留上版应用镜像，执行兼容 rollback；不可逆 schema 明确单独窗口，不自动降库。
- [ ] T22.06：配置 Prometheus/Grafana 或等价现有系统，告警 5xx/延迟/队列/连接池/攻击/磁盘/证书/备份。
- [ ] T22.07：每天基础备份并持续 WAL 归档到独立加密存储，保留至少 14 天；备份实际含必要元信息但私钥另独立受控备份。
- [ ] T22.08：新环境恢复数据库、加密密钥与签名密钥，验证账户/TOTP/Passkey/撤销；目标 RPO≤15 分钟、RTO≤60 分钟。
- [ ] T22.09：签名轮换先发布新公钥后切签，旧公钥保留会话兼容窗口；AEAD 版本化后台重加密并支持旧 key 解密。
- [ ] T22.10：生产 SMTP 配置 SPF/DKIM/DMARC，发实际邮件并检查失败处理。
- [ ] T22.11：写明单机维护中断及恢复步骤，不宣称高可用。

**验收记录：**

- 代码版本：待填写。
- 环境与时间：待填写。
- 命令退出码：待填写。
- 失败/阻塞项：待填写。
- 修复与复测：待填写。
- 任务结论：未开始。

### T23 — 第一版发布与总体验收

**前置：** T00～T22 全通过。  
**关联实现：** plan.md 的 T23.01～T23.09。  
**产物核对：** 发布清单、版本制品、验收结论、生产冒烟和观察报告。  
**验证命令/方式：** npm ci；npm run check；npm run test:unit；npm run test:integration；npm run test:e2e；npm run test:security；npm run test:accessibility；npm run build。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T23-REL-01 | 干净环境 | 完整安装/测试/构建 | 全套真实检查成功，无 skipped 关键测试 | 待填写 | 未执行 | 待提供：CI 与版本制品 |
| T23-REL-02 | 五类浏览器 | 完整主要流程、键盘与 reduced-motion | 无阻塞体验问题 | 待填写 | 未执行 | 待提供：浏览器矩阵 |
| T23-REL-03 | 生产部署 | 冒烟并观察 24 小时/高峰 | 认证/撤销/邮件健康，指标无异常 | 待填写 | 未执行 | 待提供：上线记录 |

**实现子步骤检查：**

- [ ] T23.01：从干净 checkout/锁文件安装构建，不依赖开发机缓存秘密。
- [ ] T23.02：跑全部 check/unit/integration/e2e/security/accessibility，核对必要证据。
- [ ] T23.03：手测当前稳定 Chrome/Firefox/Safari、移动 Safari/Chrome，真实 Passkey 至少一组。
- [ ] T23.04：检查生产禁用调试/seed、按钮无占位、邮件/回调/错误页真实有效。
- [ ] T23.05：确认域名、SMTP、独立备份、恢复演练及监控全部可用。
- [ ] T23.06：检查 High/Critical 为零，未解决项写明风险/影响；关键场景未通过则禁止发布。
- [ ] T23.07：生成版本、构建制品校验、变更说明、部署与 rollback 指令。
- [ ] T23.08：部署后冒烟：注册→验证→密码/MFA/Passkey→A/B SSO→全部退出→禁用检查。
- [ ] T23.09：上线观察至少 24 小时及一个实际业务高峰；异常按 runbook 回滚，不删除证据。

**验收记录：**

- 代码版本：待填写。
- 环境与时间：待填写。
- 命令退出码：待填写。
- 失败/阻塞项：待填写。
- 修复与复测：待填写。
- 任务结论：未开始。

### T24 — 后续高可用升级

**前置：** T23；不阻塞第一版。  
**关联实现：** plan.md 的 T24.01～T24.08。  
**产物核对：** 多副本拓扑、HA 数据服务、故障演练、SLO 和容量报告。  
**验证命令/方式：** 多实例 integration/e2e；HA 故障演练；npm run test:load -- --scenario=mixed。

| 案例 | 前置状态 | 操作步骤 | 预期结果 | 实际结果 | 状态 | 证据 |
|---|---|---|---|---|---|---|
| T24-HA-01 | 双 API 和 HA 数据 | 退出一个 API、切换数据库/缓存 | 按预期恢复，安全失败关闭，记录中断 | 待填写 | 未执行 | 待提供：故障报告 |
| T24-HA-02 | 请求分配不同实例 | A 实例撤销，B 实例检查与刷新 | 提交后无旧凭证有效 | 待填写 | 未执行 | 待提供：多实例测试 |
| T24-HA-03 | 监控窗口和备份 | 核算 SLO/错误预算与恢复计时 | 指标定义明确，实际 RPO/RTO 达标 | 待填写 | 未执行 | 待提供：SLO/恢复报告 |

**实现子步骤检查：**

- [ ] T24.01：至少两个 API 副本置于负载均衡，不保存实例专属认证事实。
- [ ] T24.02：配置高可用 PG/Redis，数据库状态检查不走可能滞后的只读副本。
- [ ] T24.03：统一密钥版本与共享 BFF 刷新锁，Worker 并发通过数据库租约。
- [ ] T24.04：零停机滚动部署检查新旧 schema/密钥兼容。
- [ ] T24.05：注入 API 退出、PG/Redis 切换、Worker 退出和网络超时；不得绕过认证。
- [ ] T24.06：多实例重复即时撤销和刷新竞争验收。
- [ ] T24.07：重测性能、RPO/RTO，建立 99.9% 可用性 SLO 和错误预算。
- [ ] T24.08：更新 runbook，至少覆盖一次数据库故障切换和从备份恢复。

**验收记录：**

- 代码版本：待填写。
- 环境与时间：待填写。
- 命令退出码：待填写。
- 失败/阻塞项：待填写。
- 修复与复测：待填写。
- 任务结论：未开始。

## 5. 跨任务端到端验收矩阵

这些案例在 T23 完整回归，并在相关任务提前执行。每项必须保存后端结果及必要的 UI 证据。

| 编号 | 操作 | 预期 | 关联任务 | 当前 |
|---|---|---|---|---|
| E01 | 注册→收信→点击确认→登录 | 验证前拒绝、验证后成功 | T05/T06/T17 | 未执行 |
| E02 | 已有/不存在邮箱注册与找回 | 响应不泄露存在性 | T04/T05/T07 | 未执行 |
| E03 | 连续错误登录及伪造代理 | 限流有效，不能绕过 IP 预算 | T04/T20 | 未执行 |
| E04 | 旧预认证 Cookie→成功登录 | 会话/CSRF 轮换，事务安全迁移 | T06/T10 | 未执行 |
| E05 | TOTP 开启后密码登录 | 第二因素前无普通会话 | T08 | 未执行 |
| E06 | 同 TOTP 时间步重复提交 | 最多一次成功 | T08 | 未执行 |
| E07 | 同恢复码并发消费 | 最多一次成功 | T08 | 未执行 |
| E08 | 邮箱重置 MFA 账号 | 新密码生效，MFA 保留，全会话失效 | T07/T08 | 未执行 |
| E09 | 真实 Passkey 注册与登录 | UV 成功后强认证 | T09 | 未执行 |
| E10 | 错 origin/无 UV/重复 challenge | 拒绝，不签发会话 | T09 | 未执行 |
| E11 | 登录 A→访问 B | SSO 复用，首次同意按规则 | T13 | 未执行 |
| E12 | 换回调/verifier/client/nonce | 标准拒绝，不能窃取授权 | T10/T11 | 未执行 |
| E13 | 同 code 并发交换 | 仅一个成功 | T11 | 未执行 |
| E14 | 重用轮换前 refresh | 家族撤销且提交保留 | T12 | 未执行 |
| E15 | 全设备退出→访问 A/B | 下一次身份检查无效 | T12/T13 | 未执行 |
| E16 | 管理员禁用→旧凭证/刷新 | 立即拒绝，不发新 token | T12/T14 | 未执行 |
| E17 | 普通用户请求 admin API | 403，无敏感数据 | T14/T19 | 未执行 |
| E18 | introspection 数据服务故障 | BFF 保护请求 503，无旁路 | T12/T13/T20 | 未执行 |
| E19 | SMTP 故障后恢复 | outbox 重试，不丢动作 | T05/T22 | 未执行 |
| E20 | 独立主机备份恢复 | 数据与密钥可用，实际 RPO/RTO | T22 | 未执行 |
| E21 | 签名及 AEAD 密钥轮换 | 新正常、旧窗口兼容、TOTP 不丢 | T22 | 未执行 |
| E22 | 键盘/屏幕阅读器/reduced-motion | 关键流程可完成，不依赖动效 | T16～T19 | 未执行 |
| E23 | 手机完整注册/MFA/Passkey | 无布局阻塞，输入与返回正确 | T17/T23 | 未执行 |
| E24 | GET 邮件链接/退出地址 | GET 不消费、不撤销 | T05/T07/T12 | 未执行 |
| E25 | 用户撤销与刷新并发 | 撤销提交后无新有效凭证 | T12/T20 | 未执行 |
| E26 | token 跨客户端 introspect/revoke | 不泄露、不越权撤销 | T12 | 未执行 |
| E27 | 删除/禁用最后管理员 | 拒绝并保持可管理状态 | T14 | 未执行 |
| E28 | 生产弱配置/开发 seed | 拒绝启动/运行 | T01/T03/T23 | 未执行 |

## 6. 浏览器与交互验收

| 平台 | 浏览器 | 注册/恢复 | 密码/MFA | Passkey | SSO/退出 | 键盘/布局 | 实测版本 |
|---|---|---|---|---|---|---|---|
| Windows/Linux | Chrome stable | 未执行 | 未执行 | 未执行 | 未执行 | 未执行 | 待填写 |
| Windows/Linux | Firefox stable | 未执行 | 未执行 | 未执行 | 未执行 | 未执行 | 待填写 |
| macOS | Safari stable | 未执行 | 未执行 | 未执行 | 未执行 | 未执行 | 待填写 |
| iOS | Safari stable | 未执行 | 未执行 | 未执行 | 未执行 | 未执行 | 待填写 |
| Android | Chrome stable | 未执行 | 未执行 | 未执行 | 未执行 | 未执行 | 待填写 |

自动 axe 通过不代表全部 WCAG 符合，必须人工检查焦点、错误播报、对比度、200% 放大、密码管理器、复制粘贴、系统返回和取消认证器。测试设备不可用记阻塞，不填通过。

## 7. 性能验收记录

| 项目 | 门槛 | 实际 | 结果 | 证据 |
|---|---|---|---|---|
| introspection | 300 RPS/15 分钟；p95≤100ms、p99≤250ms | 待测 | 未执行 | 待提供 |
| 普通账号查询 | 100 RPS；p95≤200ms | 待测 | 未执行 | 待提供 |
| 密码登录 | 5 RPS；p95≤1s | 待测 | 未执行 | 待提供 |
| 综合 | 300+50+5 RPS；非预期 5xx<0.1% | 待测 | 未执行 | 待提供 |
| 登录 JS gzip | ≤200KiB | 待测 | 未执行 | 待提供 |
| 首页 JS gzip | ≤300KiB | 待测 | 未执行 | 待提供 |
| 移动冷加载 | LCP≤2.5s、CLS≤0.1 | 待测 | 未执行 | 待提供 |
| 本地反馈/实验室交互 | ≤100ms / p75≤200ms | 待测 | 未执行 | 待提供 |
| 生产 RUM INP | 样本充足后 p75≤200ms | 待测 | 未执行 | 待提供 |

固定数据/硬件/浏览器/网络条件见 plan.md。必须同时记录 CPU、内存、hash 参数、连接池和错误分类。令牌池过期、压测客户机饱和、额外网络延迟应说明，不能删掉失败样本。

## 8. 恢复与运维验收

恢复记录必须包含：

1. 备份版本、加密方式、完整性校验和备份/恢复目标。
2. 可恢复时间点、事故模拟时间、数据丢失时间差，即实际 RPO。
3. 从开始恢复到业务验证完成的时间，即实际 RTO。
4. PostgreSQL、签名 key、AEAD key、配置、服务版本的恢复步骤。
5. 恢复后密码、TOTP、Passkey、OAuth、撤销检查的结果。
6. 开放端口、证书、监控、邮件与备份后续运行确认。
7. 失败项、原因、修复和下一次演练。

目标：RPO≤15 分钟、RTO≤60 分钟。单机维护中断边界必须说明；没有演练不能写目标已达到。

## 9. 发布放行与阻塞判定

| 关卡 | 要求 | 当前 |
|---|---|---|
| G0 | T00～T04 全部必要案例通过 | 已放行（当前Linux基础验收；生产未放行） |
| G1 | T05～T09 | 未放行 |
| G2 | T10～T14 | 未放行 |
| G3 | T15～T19 | 未放行 |
| G4 | T20～T22 | 未放行 |
| G5 | T23 与 E01～E28 必要场景通过 | 未放行 |
| G6 | T24 高可用独立通过 | 未放行 |

以下任意一项存在即不允许第一版发布：严重/高危安全问题；密码/MFA/Passkey 关键流程失败；撤销延迟或绕过；越权；数据恢复不可用；真实邮件无法送达；生产配置不安全；关键浏览器体验阻塞；必要测试未执行。

## 10. 缺陷模板与签字记录

```markdown
### DEF-编号
关联任务/案例：
版本与环境：
严重度：
前置条件：
复现步骤：
预期：
实际：
证据（脱敏）：
影响与发布阻塞：
修复版本：
复测命令/结果：
结论与日期：
```

发布结论由实际验收填写，不预先签字：

- 发布版本：待填写。
- 必要案例通过数/总数：待填写。
- 阻塞缺陷：待填写。
- 风险与已同意的具体例外：待填写。
- 备份恢复证据：待填写。
- 用户设计验收：待填写。
- 放行结论与日期：未放行。

## 11. 初始文档交付记录（历史）

初始规划轮只生成 plan.md 与 acceptance.md；当时系统实现任务 T01～T24 未开始。文档一致性检查已于 2026-10-07 执行，检查方式为读取磁盘中的两个 UTF-8 文件，再进行结构解析与内容比对：

| 检查 | 实际结果 |
|---|---|
| 磁盘文档与预定内容 | 两文件完整一致 |
| 任务覆盖 | 两文件均 25 个任务，T00～T24 顺序完整 |
| 实施步骤映射 | 199 个编号步骤在验收文档逐一对应 |
| 任务验收案例 | 87 个，含 T00 两个文档案例 |
| 端到端回归场景 | E01～E28，共 28 个 |
| 中文编码 | UTF-8 可读取，没有替换字符 |
| 真实完成状态 | 仅 T00 文档产物通过；T01～T24 未开始 |
| 工作区交付范围 | 仅 plan.md、acceptance.md，无代码或运行服务 |

后续证据目录、ADR 模板和自动 verify:docs 脚本属于 T01 工程初始化；它们不在本次“只写 Markdown”的交付范围。T00 下这些后续子步骤仍保持未勾选。

无 Docker 的环境事实不代表后续集成测试已通过。未执行安全、性能、浏览器、SMTP、部署和恢复测试。
