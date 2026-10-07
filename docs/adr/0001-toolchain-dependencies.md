# ADR-0001：固定 T01 工具链、依赖和开发镜像

- 状态：接受。
- 日期：2026-10-07。
- 关联任务：T00.04、T01.02～T01.04、T01.07。
- 关联验收：T01-BOOT-01；完整结果由 T01 运行证据记录。
- 替代 ADR：无。

## 背景与约束

计划已确定 Rust/Axum/Tokio、PostgreSQL 17、Redis 7.4、React 19 与 Node 编排。原文的 Windows 工具版本属于规划时的历史环境；当前工作区在 Linux/WSL2，Rust 1.98.0 已安装，Node 22.22.1、npm 9.2.0、Docker 29.1.3 和 Compose 2.40.3 可用。T01 只交付工程初始化、空路由、配置拒绝与健康检查。

## 决策

1. 保留计划明确指定的 Rust 1.98.0，workspace 使用 edition 2024、resolver 3、精确直接依赖和 Cargo.lock。SQLx 0.9.0 的 MSRV 为 1.94.0，当前 Rust 满足要求；客户端 crate 的版本不要求等于 PostgreSQL/Redis 服务主版本。
2. 根 npm workspace 使用 Node 22.22.1、package-lock.json，安装统一执行 `npm ci`。宿主 npm 为 9.2.0，固定 Node 开发镜像自带 npm 10.9.4；这两个版本均满足根 `engines`，镜像中的实际版本单独记录，不把镜像安装谎称为宿主 npm 运行。
3. 开发镜像固定 Rust 1.98.0-bookworm、Node 22.22.1-bookworm-slim、PostgreSQL 17.11-bookworm、Redis 7.4（实际二进制 7.4.11）和 Mailpit v1.31.4 的 manifest index digest。精确摘要见 [依赖证据](../evidence/T01/backend-dependencies.md)。不使用浮动 `latest`。
4. Docker Hub registry 在当前环境直连超时/不可达，实际通过 `docker.m.daocloud.io` 读取 manifest 并拉取固定摘要。镜像引用保留实际来源；不声称已从 Docker Hub 直连拉取，也不声称完成镜像签名或 OS 漏洞扫描。需更换源时重新核实内容摘要，不静默改成未知镜像。
5. Rust 开发镜像执行 `cargo build --workspace --locked`（debug），通过 Compose command 选择 server/worker。BuildKit registry/git/target cache 只保存构建数据，不从宿主复制 `.local` 或秘密。Node 开发镜像在三个 workspace 清单就绪后执行 `npm ci`，再由 command 分别启动三个 Vite 应用。
6. Compose 每服务使用唯一的应用镜像 tag，并共享 Dockerfile 与 BuildKit cache。真实并行 build 曾因两个服务向同一 tag 导出而失败；修复为独立 tag 后保留失败证据并复测，不能删除失败记录。
7. PostgreSQL、Redis、Mailpit 使用具名持久卷、实际 healthcheck；开发端口仅绑定 127.0.0.1。密钥仅在运行时只读挂载到 `/run/secrets/`，后端通过 `.local/dev.env` 获取配置。`dev:down` 不删除卷。

## 核查依据

已实际执行锁文件 metadata 解析、源码 Cargo.toml 许可证/仓库核对、crates.io 当前版本查询、GitHub 仓库维护状态查询、RustSec 锁文件扫描、镜像 registry manifest 查询及固定 digest pull。直接 Rust 依赖的源码许可证与 metadata 一致；被核查的上游仓库均未 archived/disabled。

RustSec `cargo-audit 0.22.1` 实际扫描退出 0，报告 238 个锁文件依赖、0 已知漏洞、无 warning，数据库 commit 为 `f246cde705ecb3a6b421d6d5462c6d88f317db5f`。证据为 [cargo-audit.json](../evidence/T01/cargo-audit.json)。该结论仅覆盖当前锁文件和当次公告库，不代替 T20 的安全、完整传递依赖许可证、秘密、ZAP 和协议扫描。

## 影响与后续条件

首次容器构建需要 BuildKit/buildx 与网络；后续源码变更复用构建 cache。开发镜像含编译工具，运行权限和镜像精简仍需按 T22 单独实现生产镜像，不把本开发配置宣称为生产部署。

依赖升级需更新精确版本、锁文件、核查记录和受影响测试。Passkey、Argon2id、AEAD、SMTP outbox、OIDC 客户端、指标与完整前端状态库在其对应任务引入前再核查，不为 T01 预先扩展业务功能。文档关于历史环境与当前实现状态的变更由主任务同步；后续隐性验收依赖按 T00 审计在进入相关任务前处理。

