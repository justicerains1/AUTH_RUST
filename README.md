# Rust 统一身份中心

实现依据为 [plan.md](plan.md) 和 [acceptance.md](acceptance.md)。邮箱/密码/MFA/Passkey、OAuth/OIDC、双 BFF、账号与管理后台，以及本地生产制品和运维工具已实现并逐模块推送。实体设备、规定性能环境、正式生产部署/邮件/独立恢复等仍待验收；最终自动回归实际结果见 [TEST_SUMMARY.md](TEST_SUMMARY.md)，任务状态和外部条件见 [TASK_PROGRESS.md](TASK_PROGRESS.md)。当前尚不具备生产发布条件。

## 环境

- Rust **1.98.0**（`rustup` 根据 `rust-toolchain.toml` 安装 rustfmt/clippy）。
- Node **22.22.1**，npm **9.2.0**；依赖同时兼容 Node 24，当前验收版本见证据。
- Docker Engine / Docker Desktop，Compose **v2**。Linux 当前验证 29.1.3 / 2.40.3。
- OpenSSL 3，用于本地 RSA 私钥生成。Linux 需要 C 编译工具链与 CMake（JOSE 的 aws-lc 后端）；Windows 安装 Visual Studio Build Tools C++ 和 CMake。生产镜像及发布属于 T22。

Linux 可用 rustup 官方安装程序安装 Rust；Windows 使用 rustup-init 并安装 Docker Desktop 的 Linux containers。两个平台均在仓库根目录执行以下 npm 命令。安装后确认 `cargo`、`node`、`npm`、`docker`、`openssl` 可从终端运行。Windows MSVC还需完整OpenSSL开发头文件、导入库及运行DLL；当前CI严格核对预装3.6.4并设置动态链接，见 [Windows环境配置](infra/windows/README.md)。

## 安装与开发启动

```text
npm ci
npm run dev:secrets
npm run dev:up
```

`dev:secrets` 生成 `.local/signing.pem`（RSA 3072）、`.local/encryption-keys.json`（32 字节 AEAD key）、`.local/dev.env`（随机数据库密码）。不回显秘密，已有文件会拒绝覆盖，避免破坏既有数据库和密钥。Linux 文件权限为 0600、目录 0700；Windows 将 `.local` ACL 限制到当前账号。`.local` 被 Git 与 Docker build context 忽略。

Compose 创建 PostgreSQL 17、Redis 7.4、Mailpit、API、Worker 和三个前端服务并等待健康检查。第一次下载镜像和编译会较慢。不要打印 `docker compose config` 的完整配置或上传 `.local`。开发数据库和缓存端口只绑定本机，生产开放端口另见 T22。

| 服务 | 地址 | 当前行为 |
|---|---|---|
| 身份页面 | http://localhost:5173 | 已确认设计、真实注册/验证、密码/MFA/Passkey与账号安全页面 |
| 演示 A / B | http://localhost:5174 / http://localhost:5175 | 两个独立 BFF 真实 SSO；按下文显式初始化客户端并启动 bff profile |
| API liveness | http://localhost:8080/health/live | 进程存活返回 200 |
| API readiness | http://localhost:8080/health/ready | PG/Redis 均可用 200，否则 503 |
| Worker liveness | http://localhost:8081/health/live | 真实 outbox SMTP Worker，开发环境 Mailpit |
| Mailpit | http://localhost:8025 | 开发 SMTP 捕获；SMTP 端口 1025 |

身份前端将 `/api`、`/health` 和精确OAuth协议路径转发给API（同意页面路径仍由前端处理），浏览器保持同源。开发模式由 `HOST=0.0.0.0`、`IDENTITY_API_PROXY=http://api:8080` 支持容器；在主机上可用 `npm run dev --workspace apps/identity-web`，默认绑定 127.0.0.1、代理 127.0.0.1:8080。演示应用可分别以 `--workspace apps/demo-a`、`--workspace apps/demo-b` 启动。

```text
npm run dev:down
```

停止默认保留 PostgreSQL、Redis、Mailpit 卷；不会隐式删除数据。本地秘密需与持久卷一同保留，不要重新生成数据库密码后继续使用旧卷。

## 配置规则

`.env.example` 是无秘密参考，程序通过环境变量解析，不自动加载文件。Compose 使用 `.local/dev.env`；主机运行需自行设置环境和主机连接地址。私钥、SMTP 密码、AEAD key 使用文件路径，错误只指出配置名，不输出内容。

`APP_ENV` 必须为 development/test/production。固定 `ISSUER` 只能是 origin，`RP_ID` 必须等于 host；HTTP 仅支持本机开发。生产要求 HTTPS、有效 RSA 私钥、完整版本化 AEAD key、SMTP 账号及密码文件、`SMTP_TLS=required`。禁用 TLS 校验、Cookie 降级或启用开发 seed/debug 会拒绝启动。Cookie安全策略为不变量，真实会话由后续已实现认证端点签发。

`SMTP_TLS` 补齐文档原配置表的显式 TLS 模式：`required` 要求验证证书；`disabled` 仅允许开发/test 的 localhost/loopback/Mailpit。真实 SMTP transport 实现在 T05。默认不信任转发头，可信 CIDR 仅负责解析配置，代理和限流行为在 T04 验收。

```text
cargo run --locked -p identity-server -- --check-config
cargo run --locked -p identity-server
```

`--check-config` 只验证配置，不连接依赖、不绑定端口。配置通过后健康服务可在依赖暂不可用时启动，以便 liveness/readiness 清楚区分；依赖检查超时为 2 秒。不记录请求 query、body 或认证头。

## 检查与验收

```text
npm run verify:docs
npm run test:tooling
npm run check
npm run test:unit
npm run build
npm run test:integration -- --task=T01
```

`check` 执行 fmt、Clippy `-D warnings`、TypeScript strict 和 ESLint；`build` 编译全部 Rust release 与三前端 production；`test:unit` 运行实际 Rust/Vitest 测试，零测试拒绝成功。T01 集成测试需要先 `dev:up`，验证所有服务健康、页面/代理可达、真实 production 配置拒绝，以及分别停止 PG/Redis 后的 503 和恢复 200。它只控制本项目 Compose 的开发服务，故障测试后会恢复依赖；脱敏报告保存到 `docs/evidence/T01/`。

完整integration/E2E已按固定任务manifest逐项运行，失败继续并记录退出码；security执行依赖/秘密/协议和本地生产构建ZAP。load必须明确选择场景，seed只白名单测试库。verify:openapi、数据库迁移与T16 accessibility已经有实际套件。`npm run db:migrate -- --env=development` 对本机开发目标应用SQLx校验迁移；test仅允许identity_test，production需显式环境和--allow-production。启动不会隐式迁移。测试数据 seed 禁止用于生产，T21已提供隔离十万账号/授权种子，错误环境及缺必要工具真实非零。

CI 复用相同根脚本，分别配置 Linux/Windows 构建及 Linux Compose 模块集成；产品可访问性同时安装并执行Chromium/Firefox（当前12个完整案例）。CI 配置存在不代表远端已经运行，实际执行记录以 [acceptance.md](acceptance.md) 和 [docs/evidence/T01](docs/evidence/T01) 为准。

## 目录与任务状态

`identity-core` 负责规则/配置，`identity-store` 负责 SQLx 和依赖连接，`identity-server` 负责 Axum，`identity-worker`负责真实outbox发送/重试，`identity-admin-cli` 在 T14 实现初始化，`demo-bff` 在 T13 实现两个独立客户端实例。身份前端已完成T17～T19认证/账号/同意/后台产品流程整合，A/B的实际BFF/SSO已在T13实现。T15视觉稿获用户确认，T16基础已完成。

当前通过与待验收任务见进度表；OpenAPI能力以真实任务证据为准。T17/T18认证和账号页面已实现并通过自动案例，但前置实体设备待验收；T19后台真实案例通过。最终完整测试和发布待验项目以任务进度为准。文档后续页面和跨模块验收的依赖问题已记录于 [T00 审计](docs/evidence/T00/document-audit.md)，进入相关任务前同步修订，不通过跳过测试消除依赖。

## 前端基础与数据库测试

`npm run test:integration -- --task=T03`使用真实identity_test隔离schema，验证迁移/并发/原语并只清理本轮schema；生产及非白名单先拒绝。`npm run test:accessibility`自动启动本机5190 Vite组件页并运行Playwright+axe，浏览器先执行`npx playwright install chromium`；development `/dev/components`不进入production构建。T16_BASE_URL可显式本机测试地址，源码里guard仅页面体验，API独立校验权限。

## 完整检查与总结

`npm run test:full`依次执行npm ci、文档/契约/工具检查、check、unit、全部integration/e2e/security/accessibility、build以及账号/管理交互实验。任一失败仍执行剩余项，整体返回非零，生成`TEST_SUMMARY.md`及带时间戳的`docs/evidence/full-test/`报告。原始进程输出只保存到受限的`.local/full-test/`，避免把秘密写入提交。缺必要套件或工具会如实失败；此命令不代替性能、真实设备或生产恢复验收。

运行完整安全与运维检查需准备固定 Gitleaks、cargo-deny、ZAP/Java 和 age 工具；版本/摘要见 [安全工具记录](docs/evidence/T20/scan-tool-versions.json) 和 [age来源与默认路径](docs/evidence/T22/age-source.md)。T22 会真实运行隔离 PG 备份/WAL 测试以及本地 exporter→Prometheus→Alertmanager 接收链；后者需固定 Prometheus3.15.0、node_exporter1.12.1、Alertmanager0.34.1，官方来源、摘要、默认路径见[监控链复测](docs/evidence/T22/monitoring-chain/test-summary.md)。缺必要工具不会跳过返回成功。


## 双 BFF 演示开发环境（T13）

先启动基础服务，再显式迁移并注册本地客户端：

```text
npm run dev:up
npm run db:migrate -- --env=development
npm run dev:clients
docker compose --env-file .local/dev.env -f infra/compose.dev.yaml --profile bff up -d --wait --build
```

`dev:clients`只读取受控本地development配置和identity_development；已有客户端需秘密摘要及完整回调一致，命令不轮换或覆盖秘密。A/B秘密存.local/bff的0600文件，请保留并勿提交；CLI独立支持显式identity_test，production一律拒绝。

BFF的bff profile启动两个独立Rust实例，网页分别5174/5175、BFF8082/8083。浏览器仅持本应用HttpOnly会话；保护请求每次查询身份平台当前状态，PostgreSQL共享锁串行刷新。平台退出需身份页确认当前会话，无ID token hint外回。完整接入与故障处理见[接入指南](docs/bff-integration.md)。T13本地真实验收通过，生产与全面发布仍以进度表和实际证据为准。


## 首个管理员初始化（T14）

先按对应环境完成配置与迁移，再执行：

```text
cargo run --locked -p identity-admin-cli -- bootstrap --email operator@example.com
```

TTY会隐藏密码输入；自动化可通过受控stdin传入单行密码，密码不得作为参数或环境变量。命令只在数据库没有管理员成员时成功，多个初始化并发仅一个提交。新用户由受控初始化验证；已有用户必须已验证、active且密码正确，不重置已有密码。未配置因素的首个管理员只可访问本人绑定必需流程；绑定TOTP/Passkey后，管理端点仍要求最近五分钟强认证。

后续管理员仅通过受保护管理API授予已验证且有因素的用户。删除、禁用或移除最后认证因素均保留至少一名可用管理员。用户/客户端/管理员/审计列表分页，客户端秘密只在创建或轮换成功时展示一次；管理UI在T19实现。CLI测试schema参数仅显式identity_test允许，生产不得使用测试开关。


## 负载、安全与运维模块

`npm run test:load -- --scenario=introspection|account|password|mixed` 使用真实测试服务，两分钟预热、十五分钟测量，失败报告保留。`npm run seed:acceptance`验证十万账号、二十客户端、十万授权种子后清理本次schema，禁止生产；测试秘密仅.local受限文件。前端独立性能测量：`node tests/performance/frontend.mjs`，固定移动网络、Chrome及五次冷加载；`npm run test:interactions`另跑真实账号/管理六场景各五本地和五实验室样本，构建production dist后执行，完整回归必跑。

生产镜像、Caddy/Compose、备份/WAL/恢复和密钥维护操作稿位于infra及docs/runbooks。它们已做相应本地验证，实际域名/SMTP/独立备份/告警和生产恢复仍未验收。内部API/Worker `/metrics`需要配置受限METRICS_TOKEN_FILE供私网采集；未配置时仅loopback可读，公网Caddy不代理。密钥维护不可删除仍被历史备份依赖的旧版本。


## 本地运维与补充验收

`npm run test:integration -- --task=T21`执行精确SQL取证和实际接口查询次数，完整性能仍需显式`test:load`的原120秒预热/900秒测量。`npm run test:capacity`为独立短阶梯（355/710/1420/2840 RPS混合），超载失败保持非零，不能代替参考硬件或十五分钟验收；方法见[容量记录](docs/evidence/T21/capacity-current-summary.md)。

T22集成还包含受控主机指标/告警规则、签名先发布公钥与真实BFF刷新、完整身份加密备份恢复以及本地发布/迁移失败中止/兼容回滚。工具固定版本与权限见[主机指标](docs/runbooks/host-metrics.md)、[备份恢复](docs/runbooks/backup-recovery.md)和[单机部署](docs/runbooks/single-host-deployment.md)。生产发布脚本需要明确`--allow-production`及实际准备的受控配置，Git推送和本地演练不启动生产。

后续[高可用升级设计](docs/runbooks/high-availability-upgrade.md)已补规划；它没有实现T24的HA数据服务、滚动升级或故障切换，实际阶段仍须满足T23前置及独立验收。

每日备份样例与保守基础备份catalog/保留工具见[备份操作稿](docs/runbooks/backup-recovery.md)，默认只生成计划，缺真实窗口前恢复锚点或分叉/损坏/pin变化时拒清理，WAL/历史/密钥一律保留；不代表已安装生产timer或已有14天恢复历史。
