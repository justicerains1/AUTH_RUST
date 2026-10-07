# T01 Rust 与开发镜像依赖核查

- 执行日期：2026-10-07（UTC）。
- 工作目录：`/root/code/rust/auth_rust`。
- 执行者：Codex 文档/基础设施代理；RustSec 命令由主代理执行并提供实际 JSON。
- 关联步骤：T01.02、T01.04。
- 本记录只证明已实际完成的核查和镜像拉取，不替代 T01 启动、配置负向与依赖故障验收。

## Rust 版本与依赖来源

实际 `/root/.cargo/bin/cargo --version`：`cargo 1.98.0 (797e8a9bc 2026-08-05)`。固定 Rust 镜像内实际 `rustc --version`：`rustc 1.98.0 (88d9e12ae 2026-08-18)`。

`cargo metadata --locked --format-version 1` 已退出 0，解析到 238 个 package（包含 workspace 包），锁文件内容 SHA-256：`079595495f6bcad98a077a3745dbd554e3b0f10f1a8114755a1c3201db24d9dc`。随后真实读取已下载 crate 的源码 Cargo.toml，核对 `license`、`repository` 与 metadata；以下条目全部一致。

| crate | 使用版本 | 源码许可证 | MSRV | 上游源码仓库 |
|---|---|---|---|---|
| axum | 0.8.9 | MIT | 1.80 | https://github.com/tokio-rs/axum |
| base64 | 0.23.1 | MIT OR Apache-2.0 | 1.71.0 | https://github.com/marshallpierce/rust-base64 |
| ipnet | 2.12.2 | MIT OR Apache-2.0 | 未声明 | https://github.com/krisprice/ipnet |
| jsonwebtoken | 11.1.0 | MIT | 1.88.0 | https://github.com/Keats/jsonwebtoken |
| redis | 1.7.1 | BSD-3-Clause | 1.88 | https://github.com/redis-rs/redis-rs |
| serde | 1.0.229 | MIT OR Apache-2.0 | 1.56 | https://github.com/serde-rs/serde |
| serde_json | 1.0.151 | MIT OR Apache-2.0 | 1.71 | https://github.com/serde-rs/json |
| sqlx | 0.9.0 | MIT OR Apache-2.0 | 1.94.0 | https://github.com/launchbadge/sqlx |
| tokio | 1.53.2 | MIT | 1.71 | https://github.com/tokio-rs/tokio |
| tracing | 0.1.44 | MIT | 1.65.0 | https://github.com/tokio-rs/tracing |
| tracing-subscriber | 0.3.23 | MIT | 1.65.0 | https://github.com/tokio-rs/tracing |
| url | 2.5.8 | MIT OR Apache-2.0 | 1.63 | https://github.com/servo/rust-url |
| zeroize | 1.9.1 | Apache-2.0 OR MIT | 1.85 | https://github.com/RustCrypto/utils |

MSRV 未声明的 ipnet 不推定任意 toolchain 兼容；当前构建验证由 T01 构建证据证明。以上为本阶段顶层 Rust 依赖核查，不是全部传递依赖许可证放行。

## 实际维护状态查询

实际请求 `https://crates.io/api/v1/crates/{name}`；上述 13 个条目的 `max_stable_version` 均等于使用版本。实际请求各上游 `https://api.github.com/repos/{owner}/{repo}`；查询的 12 个唯一仓库均为 `archived=false`、`disabled=false`。

| 仓库 | 实际最近 pushed_at（UTC） |
|---|---|
| tokio-rs/axum | 2026-10-06 20:21:10 |
| marshallpierce/rust-base64 | 2026-08-18 23:08:58 |
| krisprice/ipnet | 2026-09-06 05:42:24 |
| Keats/jsonwebtoken | 2026-10-04 13:33:29 |
| redis-rs/redis-rs | 2026-10-07 10:12:37 |
| serde-rs/serde | 2026-09-22 03:42:04 |
| serde-rs/json | 2026-08-08 06:50:26 |
| launchbadge/sqlx | 2026-10-04 03:49:35 |
| tokio-rs/tokio | 2026-10-07 06:18:13 |
| tokio-rs/tracing | 2026-05-30 06:42:00 |
| servo/rust-url | 2026-07-31 14:14:29 |
| RustCrypto/utils | 2026-10-06 17:43:25 |

GitHub 仓库 license API 对多许可证/monorepo 可能返回单一值、NOASSERTION 或空值，因此许可证结论依据下载源码中的 Cargo.toml 声明，不用仓库 API 猜测条目许可证。仓库未归档只证明当前维护状态标志，不能单独证明无安全缺陷。

## RustSec 实际扫描

| 项目 | 实际结果 |
|---|---|
| 工具 | cargo-audit 0.22.1 |
| 命令 | `cargo audit --json` |
| 退出码 | 0（主代理实际执行） |
| 锁文件 dependency-count | 238 |
| vulnerabilities.found / count | false / 0 |
| warnings | 空对象 |
| 公告数 | 1293 |
| 公告库 commit | f246cde705ecb3a6b421d6d5462c6d88f317db5f |
| 公告库 last-updated | 2026-10-07T10:23:44+02:00 |
| 原始产物 | [cargo-audit.json](cargo-audit.json) |
| 原始产物 SHA-256 | cef74b5f8759f51167eafbda5d25364fbb03af5cfe5daf200d7f8e2c22b3e8e4 |

未执行镜像 OS 漏洞扫描、全量传递依赖许可证扫描、ZAP、协议负向矩阵和生产安全审计；这些不能在 T01 被标记通过。后续每次引入认证库时重新核查该依赖与锁文件。

## 镜像 registry 与实际 pull

Docker 实际版本 29.1.3，Compose 2.40.3。Docker Hub 直连实测失败：`docker manifest inspect docker.io/library/rust:1.98.0-bookworm` 和 Node 对应命令返回 registry ping deadline exceeded；Hub API 返回 Network unreachable。真实失败没有记为成功。

实际通过 `docker.m.daocloud.io` 的 V2 registry 鉴权与 manifest API 取得以下 index digest，再逐个执行 `docker pull <source>:<tag>@<digest>`。5 个 pull 全部退出 0，输出 digest 与查询一致。公共 registry 临时 bearer 凭据未保存到本证据。

| 镜像实际来源与固定 tag | manifest index digest | pull |
|---|---|---|
| docker.m.daocloud.io/library/rust:1.98.0-bookworm | sha256:82150a52ec202c1b14d7817e14516c392bb7f5cfebd88f1ed531cb37ebd39922 | 退出 0 |
| docker.m.daocloud.io/library/node:22.22.1-bookworm-slim | sha256:4f77a690f2f8946ab16fe1e791a3ac0667ae1c3575c3e4d0d4589e9ed5bfaf3d | 退出 0 |
| docker.m.daocloud.io/library/postgres:17.11-bookworm | sha256:3645570cccdfa447589da9f57dd740faa29b30938e861289a5574b6ca6b03826 | 退出 0 |
| docker.m.daocloud.io/library/redis:7.4 | sha256:4fa24486b8bcca8eec45ee0eb166edc674795e53a2b53d1a9ef263eecebaac85 | 退出 0 |
| docker.m.daocloud.io/axllent/mailpit:v1.31.4 | sha256:b68349e3a014b90c5610bfb26b2ae36f3892d7b8cf25ee140c6c71c98d2fcf48 | 退出 0 |

本机 linux/amd64 子 manifest 摘要：

| 镜像 | amd64 digest |
|---|---|
| Rust | sha256:4e4a7e7939c17991ab35f2b8c2e67593980f771d28f6b1254b1850f860fd0c7f |
| Node | sha256:af5818e10f6294a719b4314f34ec03d8e8ad8f571a8d23742418790e6ebb5c90 |
| PostgreSQL | sha256:66aafa11cf15800a3c94763f7e11d1e7b2e37e5e84bc4ce027cb6e1e4bfaf4df |
| Redis | sha256:cd745595f143052dd6a743bc5651d3ce4b03979fe5c99c7fcfab73461f6f217b |
| Mailpit | sha256:c8e498023104710cd71a7bb1856a51f2183ff0dc1ea07675067a38ecda08428f |

实际在无网络一次性容器中查询版本，命令均退出 0：

- Rust：rustc 1.98.0 / cargo 1.98.0。
- Node：v22.22.1 / npm 10.9.4。
- PostgreSQL：17.11（Debian 17.11-1.pgdg12+2）。
- Redis：7.4.11；Compose 使用 `7.4@digest` 固定实际内容，不随 tag 后续更新。
- Mailpit：官方 GitHub latest release 实际为 v1.31.4，发布于 2026-10-03 04:36:58 UTC；镜像 `--help` 与镜像 Healthcheck 均确认 `/mailpit readyz` 命令存在。

官方来源核对：PostgreSQL 的 `docker-library/postgres/17/bookworm/Dockerfile` 实际 PG_VERSION 为 17.11-1.pgdg12+2；Mailpit release API 为 `https://api.github.com/repos/axllent/mailpit/releases/latest`。Redis upstream 仓库当前 Dockerfile 仍显示 7.4.8，实际 registry 的 7.4 镜像二进制为 7.4.11；本记录按拉取内容记录，不把不一致的源文件版本写成实际镜像版本。

## 开发配置核查与适用边界

`docker compose --env-file .local/dev.env -f infra/compose.dev.yaml config --quiet` 在生成配置后及独立镜像 tag 修复后均真实退出 0，没有输出秘密。

开发 Compose 为 PG/Redis/Mailpit 配置实际 healthcheck、具名卷及 localhost 端口，API readiness 查询实际 PG/Redis；Worker 的 T01 liveness 只证明进程存活，不宣称 outbox 邮件投递已实现。三个 Vite 服务通过真实页面请求 healthcheck，身份网页使用固定容器代理地址 `http://api:8080`。

Rust 开发 Dockerfile 固定 base digest，执行 locked workspace debug build，通过 BuildKit cache 重用 registry/git/target，最后安装所需 binary；Node Dockerfile 执行真实 `npm ci`。`.dockerignore` 排除 `.local`、环境文件、私钥、宿主 node_modules/target 与证据；实际秘密仅运行时只读挂载。

启动和构建由主代理继续执行，实际构建失败与修复在其 commands 证据中保留。仅配置解析与 pull 成功不代表 T01-BOOT-01～03 已全部通过，也不代表 G0 或后续生产关卡放行。
