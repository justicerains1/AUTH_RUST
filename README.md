# Rust 统一身份中心

实现依据为 [plan.md](plan.md) 和 [acceptance.md](acceptance.md)。按任务依赖推进，每个任务写实际运行证据。已完成 **T01～T05、T15～T16**：工程、配置与健康、接口契约、数据库与事务原语、安全基础、经确认的设计及前端基础；T05注册、验证邮件和Worker已通过真实验收。按用户要求，已停止继续推进，等待下一次指令。任务状态见 [TASK_PROGRESS.md](TASK_PROGRESS.md)。账号、MFA、OAuth/OIDC、后台和生产部署仍由后续任务实现，当前版本不能用于用户身份认证。

## 环境

- Rust **1.98.0**（`rustup` 根据 `rust-toolchain.toml` 安装 rustfmt/clippy）。
- Node **22.22.1**，npm **9.2.0**；依赖同时兼容 Node 24，当前验收版本见证据。
- Docker Engine / Docker Desktop，Compose **v2**。Linux 当前验证 29.1.3 / 2.40.3。
- OpenSSL 3，用于本地 RSA 私钥生成。Linux 需要 C 编译工具链与 CMake（JOSE 的 aws-lc 后端）；Windows 安装 Visual Studio Build Tools C++ 和 CMake。生产镜像及发布属于 T22。

Linux 可用 rustup 官方安装程序安装 Rust；Windows 使用 rustup-init 并安装 Docker Desktop 的 Linux containers。两个平台均在仓库根目录执行以下 npm 命令。安装后确认 `cargo`、`node`、`npm`、`docker`、`openssl` 可从终端运行。

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
| 身份页面 | http://localhost:5173 | 已确认设计的路由/组件、注册与邮箱验证；密码登录待T06 |
| 演示 A / B | http://localhost:5174 / http://localhost:5175 | 开发应用空路由，尚无 SSO |
| API liveness | http://localhost:8080/health/live | 进程存活返回 200 |
| API readiness | http://localhost:8080/health/ready | PG/Redis 均可用 200，否则 503 |
| Worker liveness | http://localhost:8081/health/live | 真实 outbox SMTP Worker，开发环境 Mailpit |
| Mailpit | http://localhost:8025 | 开发 SMTP 捕获；SMTP 端口 1025 |

身份前端将 `/api`、`/health` 转发给 API，浏览器保持同源。开发模式由 `HOST=0.0.0.0`、`IDENTITY_API_PROXY=http://api:8080` 支持容器；在主机上可用 `npm run dev --workspace apps/identity-web`，默认绑定 127.0.0.1、代理 127.0.0.1:8080。演示应用可分别以 `--workspace apps/demo-a`、`--workspace apps/demo-b` 启动。

```text
npm run dev:down
```

停止默认保留 PostgreSQL、Redis、Mailpit 卷；不会隐式删除数据。本地秘密需与持久卷一同保留，不要重新生成数据库密码后继续使用旧卷。

## 配置规则

`.env.example` 是无秘密参考，程序通过环境变量解析，不自动加载文件。Compose 使用 `.local/dev.env`；主机运行需自行设置环境和主机连接地址。私钥、SMTP 密码、AEAD key 使用文件路径，错误只指出配置名，不输出内容。

`APP_ENV` 必须为 development/test/production。固定 `ISSUER` 只能是 origin，`RP_ID` 必须等于 host；HTTP 仅支持本机开发。生产要求 HTTPS、有效 RSA 私钥、完整版本化 AEAD key、SMTP 账号及密码文件、`SMTP_TLS=required`。禁用 TLS 校验、Cookie 降级或启用开发 seed/debug 会拒绝启动。Cookie 的安全策略作为不变量保留；T01 不签发会话 Cookie。

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

未实现的 integration 任务、全量 e2e/security/load 和 seed 返回非零并指出所属任务；verify:openapi、数据库迁移与T16 accessibility已经有实际套件。`npm run db:migrate -- --env=development` 对本机开发目标应用SQLx校验迁移；test仅允许identity_test，production需显式环境和--allow-production。启动不会隐式迁移。测试数据 seed 禁止用于生产，T21再补完整种子，未实现命令仍非零。

CI 复用相同根脚本，分别配置 Linux/Windows 构建，以及 Linux Compose T01 集成任务。CI 配置存在不代表远端已经运行，实际执行记录以 [acceptance.md](acceptance.md) 和 [docs/evidence/T01](docs/evidence/T01) 为准。

## 目录与下一任务

`identity-core` 负责规则/配置，`identity-store` 负责 SQLx 和依赖连接，`identity-server` 负责 Axum，`identity-worker` 后续处理 outbox，`identity-admin-cli` 在 T14 实现初始化，`demo-bff` 在 T13 实现两个独立客户端实例。三个前端保留空路由，T15视觉稿已获用户确认，T16组件与路由基础已完成；真实业务页在T17～T19接入。

T01～T04已通过当前Linux真实验收；完整OpenAPI包括未来端点契约，已实现端点以任务验收记录为准。当前只提交已完成代码与进度记录，不开始T06。文档后续页面和跨模块验收的依赖问题已记录于 [T00 审计](docs/evidence/T00/document-audit.md)，进入相关任务前同步修订，不通过跳过测试消除依赖。

## 前端基础与数据库测试

`npm run test:integration -- --task=T03`使用真实identity_test隔离schema，验证迁移/并发/原语并只清理本轮schema；生产及非白名单先拒绝。`npm run test:accessibility`自动启动本机5190 Vite组件页并运行Playwright+axe，浏览器先执行`npx playwright install chromium`；development `/dev/components`不进入production构建。T16_BASE_URL可显式本机测试地址，源码里guard仅页面体验，API独立校验权限。
