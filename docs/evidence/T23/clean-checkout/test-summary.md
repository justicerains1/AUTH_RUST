# T23 干净 checkout 子模块最终结果

2026-10-08，实际验证提交 `319f30c4e7de8090cb7055011acc2471530f4e5c`。从新的 detached worktree 开始，node_modules、target、三前端 dist 与 `.local` 均不存在；安装使用空 npm cache/userconfig，Cargo 使用全新独立 target，仅共享现有 Rustup 工具链和 Cargo 注册表源码缓存。

[最终原报告](2026-10-08T12-36-47-083Z.json) 七阶段真实退出码全部 0：`npm ci`、verify:docs、verify:openapi、test:tooling、check、test:unit、build。文档25任务/199步骤/87任务案例；OpenAPI59操作/128schema/601examples；66工具测试0failed/0skipped；Rust58单元0ignored，身份前端59、A/B各6单元全部通过。

源码开始/结束均 clean；没有 dev.env、签名或 AEAD 身份秘密文件。locked release 七个 Rust 可执行文件与三前端共53产物的 SHA-256 已逐个读取并复核一致，必需产物无缺失；校验和、工具版本及两文档/锁文件校验见原报告。原始诊断只存忽略目录0600文件。

此前 [add1400 首轮](2026-10-08T12-11-26-363Z.json) 因三个正常流程 fixture 固定12:05截止而整体退出1，保留 [失败定位](2026-10-08T12-11-26-363Z-failure-analysis.md)。修复仅用测试开始时间生成有限有效窗口，未放宽生产过期规则。复测是新 checkout、新依赖安装、新 target，未把旧失败覆盖成通过。一次提交 SHA 拷贝错误在任何测试前被 guard 拒绝，也作为 [零执行设置失败](2026-10-08T12-39-retry2-setup-error.json) 保留，不作测试结果。

中间提交 `1c0c9eaace5c8ddb9fd77803d6bc42b5abf967f8` 的 [Git 对照](source-equivalence-319f30c-1c0c9ea.json) 确认当时仅改 edge Dockerfile 与证据 Markdown。[最终修复提交 f5e7640 的对照](source-equivalence-319f30c-f5e7640.json) 确认生产 Rust、前端、锁文件、迁移和data仍相同，但 `t13_bff.rs` 测试harness、T13 E2E 与integration入口已经变化：将组合故障拆为独立案例，总六案例保留真实状态/撤销/刷新故障断言并实际通过，见 [独立运行](../../tooling/t13-fault-isolation/test-summary.md)。因此不能把最终全部tests声称与干净构建提交相同。

本干净结果仍只标实际执行的319f30c提交，不能声称最终SHA本身在该worktree执行了七stage。edge最终镜像的源码归档构建与规定运行限制见 [独立制品报告](../final-artifacts.md)，不从本机release/dist校验推定相同镜像digest。最终提交的全部harness/E2E/安全回归由根最新完整运行报告另行记录。

这是安装/静态/单元/构建子模块通过，不等于完整 T23、远端 CI 或生产发布。根的完整 integration/e2e/security/accessibility/fault 为独立运行；真实 Passkey、五类浏览器人工矩阵、正式生产域名/邮件/独立恢复/密钥分发/告警和24小时高峰观察仍按 [剩余验收](../remaining-verification.md) 执行。
