# T23 源码、镜像与 E28 最后审计边界

2026-10-08 UTC。此次只读审计未发现仓库或 `/root/code/rust/auth_rust` 祖先目录中的 AGENTS.md。实施/验收仍以 [plan.md](../../../plan.md) 和 [acceptance.md](../../../acceptance.md) 为准；E28 要求生产弱配置和开发 seed 实际拒绝，不能用构建通过推定这项通过。

## 干净 checkout 的证明

每轮使用明确 SHA 的 detached worktree，无新远端分支。开始前核对 tracked 源码 clean，node_modules、target、三前端 dist 和 `.local` 全部不存在；独立空 npm cache/userconfig 和全新 Cargo target 运行锁文件安装、两文档/OpenAPI/tooling、静态检查、lib/bin 单元及 release/三前端构建。仅共享已安装 Rustup 工具链和 Cargo 注册表源码缓存，不共享原 target、dist、node_modules 或身份秘密。

最终报告需要记录：目标 SHA、开始/结束 clean、工具版本、锁文件与两 md 的 SHA-256、真实退出码和单测计数、全部生成制品 SHA-256；不创建 dev.env/签名/AEAD 文件，不启动 Compose。单元为 `--lib --bins`，真实数据库/浏览器/故障由根独立完整测试执行，不能把这份静态子模块当成完整 T23。

首轮 [add1400 报告](clean-checkout/2026-10-08T12-11-26-363Z.json) 真实整体退出 1：生产过期规则正确拒绝固定到 12:05 UTC 的测试 fixture，三个前端单元失败，其他阶段及53制品构建通过。[失败定位](clean-checkout/2026-10-08T12-11-26-363Z-failure-analysis.md) 保留，修复提交的复测必须另存新报告。

## E28 的真实执行目标

| 检查 | 实际入口和拒绝方式 | 需要保留的证据 |
|---|---|---|
| 生产弱配置 | [T01 configCases](../../../tests/integration/T01.mjs) 从当前运行 API 容器取 `.Image`，以独立受控 env/密钥挂载调用该镜像 `identity-server --check-config`；HTTP issuer、错 RP、弱 Cookie、plaintext SMTP、缺/坏密钥、DEV_SEED、DEBUG_ROUTES 等非零且只识别配置字段。 | 最终按最新源码 `dev:up --build` 并等待 running/healthy 后执行 T01；记录实际容器镜像 ID。仅复用旧容器不能证明当前源码规则。 |
| 测试/迁移目标拒绝 | [T03](../../../tests/integration/T03.mjs) 以非可连接目标执行 production、错误库、dbname override；要求非零且不打印 sentinel，不连接其他库。 | 最终真实 suite 退出码与 PASS 条目；无 ignored 或缺环境假通过。 |
| 接收负载 seed | [load runner](../../../tests/load/run.mjs) 在读本地配置/创建密钥/schema 前验证显式 testOnly、禁止 query override、白名单 identity_test/loopback；根入口拒绝额外生产 selector。 | 生产/错误目标真实拒绝记录和 fixture 数量；load-generator 纯逻辑单元不能替代数据库目标检查。 |
| 最终生产制品 | [Rust production Dockerfile](../../../infra/Dockerfile.prod) locked release 产物需包含 server/worker/demo-bff/admin/migrate/identity-keys；[edge](../../../infra/Dockerfile.edge.prod) 从当前锁文件与三前端源码构建 dist。 | 最后源码修复后重建生产镜像并记录实际 digest/config/read-only/二进制检查；早期 T22 review digest 仅对应当时构建快照。 |

`identity-demo-clients` 是开发验收初始化程序，可以作为 workspace release 构建产物记录，但不复制生产 runtime；生产客户端由受控管理员 API/CLI 管理。生产维护必须包含 `identity-keys`，否则 runbook 的版本化密钥维护命令在制品中不可执行。

## 发布不能推定的部分

当前生产 Compose 和操作稿的 WAL/独立备份配置有效性应按最终提交单独验证。合成 example 域名、配置解析、临时 PG 小样本 PITR 和本地 AEAD 维护不证明真实域名/TLS、每日独立备份保留、完整账号因素恢复、生产签名公钥分发、RPO/RTO 或告警送达。

Linux Playwright WebKit 自动化不等于 macOS/iOS Safari 实测，Linux Firefox 自动化也不填手机平台矩阵。实体 Passkey、五类浏览器人工主要流程和至少24小时/真实高峰观察仍按 [剩余清单](remaining-verification.md) 验收，不能由最终自动测试结果直接释放关卡。
