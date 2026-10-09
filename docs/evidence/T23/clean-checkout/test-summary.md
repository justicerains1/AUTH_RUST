# T23 干净 checkout 子模块最终结果

2026-10-09新增最新独立检出实测提交 `895c16c8a25243bd5c1292276bb73752a208f5a4`，02:56:15～03:04:53 UTC完成，七阶段全部退出0，见[最新结构报告](20261009-2026-10-09T02-56-15-708Z.json)与[命令摘要](20261009-2026-10-09T02-56-15-708Z.md)。工具70测试0failed/0skipped；Rust63 lib/bin单元0ignored，身份前端60、A/B各6单元通过，release七Rust命令和三前端构建成功，共53项制品。

新detached worktree为 `/root/code/rust/auth_rust_clean_t23_20261009`，开始前node_modules/target/dist/.local全无；空npmcache/userconfig、独立Cargo target和build jobs2，仅共享工具链及Cargo注册表源码缓存。运行前后Git均clean，1249项受跟踪源码SHA全部一致，53项制品二次读取摘要一致；身份秘密文件0、必需产物缺失0。没有复制本机dev.env/签名/AEAD文件，没有Compose、集成、浏览器或生产部署。前六阶段分别14.45/0.11/0.50/2.67/116.05/130.05秒，release与前端build252.57秒。

根代理随后在 `a8ea705` 执行最新完整回归，属于另一份实际结果；这次干净七阶段仅证明895c16c，不冒a8ea705自身在该worktree执行。895→a8ea的crate/app/data/migration/锁文件/Dockerfile/Caddy/Compose生产与构建输入Git diff为空，后续新增专属五服务部署测试/入口与证据不在此静态子模块执行范围。最新全量和远端CI仍按各自提交报告。下面保留早期所有干净检出与失败历史，不覆盖其含义。

最新一次验证提交为 `ffe3b9878ef729e7ca6966186657e00da4886d99`，2026-10-08 19:25:06～19:33 UTC（上海日期2026-10-09）。[本次原报告](2026-10-08T19-25-06-451Z.json) 和 [命令摘要](2026-10-08T19-25-06-451Z.md) 七阶段实际退出码全部0：空缓存`npm ci`、文档/OpenAPI、工具测试、静态检查、单元和release/三前端构建。工具70测试0failed/0skipped；Rust63 lib/bin单元0ignored，身份前端60、A/B各6单元全部通过。数量来自各crate实际结果求和，没有用预计数。

本次使用另一全新detached worktree `auth_rust_clean_t23_20261008_retry3`，开始前node_modules/target/三dist/.local全部不存在；Cargo并行任务2，独立target及空npm cache/userconfig，只共享现有Rustup工具链与Cargo注册表源码缓存。源码开始/结束均clean，身份秘密文件0，必需产物无缺；53份制品SHA-256实盘逐个复核一致，锁文件与两md摘要也已核对。没有启动Compose或数据库/浏览器测试，完整回归由根另行记录；这次结果直接对应ffe3b98，早期提交的源码等价说明不替代此轮实际验证。

以下保留早期独立验收、失败与修复历史，不把它们覆写成当前结果。

2026-10-08，实际验证提交 `319f30c4e7de8090cb7055011acc2471530f4e5c`。从新的 detached worktree 开始，node_modules、target、三前端 dist 与 `.local` 均不存在；安装使用空 npm cache/userconfig，Cargo 使用全新独立 target，仅共享现有 Rustup 工具链和 Cargo 注册表源码缓存。

[最终原报告](2026-10-08T12-36-47-083Z.json) 七阶段真实退出码全部 0：`npm ci`、verify:docs、verify:openapi、test:tooling、check、test:unit、build。文档25任务/199步骤/87任务案例；OpenAPI59操作/128schema/601examples；66工具测试0failed/0skipped；Rust58单元0ignored，身份前端59、A/B各6单元全部通过。

源码开始/结束均 clean；没有 dev.env、签名或 AEAD 身份秘密文件。locked release 七个 Rust 可执行文件与三前端共53产物的 SHA-256 已逐个读取并复核一致，必需产物无缺失；校验和、工具版本及两文档/锁文件校验见原报告。原始诊断只存忽略目录0600文件。

此前 [add1400 首轮](2026-10-08T12-11-26-363Z.json) 因三个正常流程 fixture 固定12:05截止而整体退出1，保留 [失败定位](2026-10-08T12-11-26-363Z-failure-analysis.md)。修复仅用测试开始时间生成有限有效窗口，未放宽生产过期规则。复测是新 checkout、新依赖安装、新 target，未把旧失败覆盖成通过。一次提交 SHA 拷贝错误在任何测试前被 guard 拒绝，也作为 [零执行设置失败](2026-10-08T12-39-retry2-setup-error.json) 保留，不作测试结果。

中间提交 `1c0c9eaace5c8ddb9fd77803d6bc42b5abf967f8` 的 [Git 对照](source-equivalence-319f30c-1c0c9ea.json) 确认当时仅改 edge Dockerfile 与证据 Markdown。[最终修复提交 f5e7640 的对照](source-equivalence-319f30c-f5e7640.json) 确认生产 Rust、前端、锁文件、迁移和data仍相同，但 `t13_bff.rs` 测试harness、T13 E2E 与integration入口已经变化：将组合故障拆为独立案例，总六案例保留真实状态/撤销/刷新故障断言并实际通过，见 [独立运行](../../tooling/t13-fault-isolation/test-summary.md)。因此不能把最终全部tests声称与干净构建提交相同。

本干净结果仍只标实际执行的319f30c提交，不能声称最终SHA本身在该worktree执行了七stage。edge最终镜像的源码归档构建与规定运行限制见 [独立制品报告](../final-artifacts.md)，不从本机release/dist校验推定相同镜像digest。最终提交的全部harness/E2E/安全回归由根最新完整运行报告另行记录。

随后补齐 E08 同一 MFA 用户重置、E12 错误 nonce 与 E16 同一用户禁用后的凭证组合，最终测试提交为 `bc742d10368d8255e3e54cb56539ad207cbb527b`。[新 Git 对照](source-equivalence-319f30c-bc742d1.json) 确认生产 Rust/前端/锁文件/迁移/data仍相同，T07/T13/T14测试harness、T07/T13入口、T13 E2E及T11互操作测试已变化。新增案例已分别真实局部执行：E12 用同一实际兑换的有效RS256响应，错误expectedNonce准确nonce claim拒绝后正确nonce成功；E16 先证实同一fresh access/refresh active，再实际管理员禁用后拒绝同会话、两原token与current refresh，且无新token。它们不由此前319f30c干净测试或更早全套结果推定通过；包含全部新增测试的最终完整运行按根报告记录，本次只读审计时尚在运行，随后bc742d1最终完整十一阶段于2026-10-08T13:56:28Z全0，见[永久全量结果](../../full-test/2026-10-08T13-41-49-384Z.md)。

这是安装/静态/单元/构建子模块通过，不等于完整 T23、远端 CI 或生产发布。根的完整 integration/e2e/security/accessibility/fault 为独立运行；真实 Passkey、五类浏览器人工矩阵、正式生产域名/邮件/独立恢复/密钥分发/告警和24小时高峰观察仍按 [剩余验收](../remaining-verification.md) 执行。
