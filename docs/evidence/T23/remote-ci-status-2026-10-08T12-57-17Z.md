# T23 远端 CI 状态观察

实际观察 UTC：2026-10-08T12:57:17.004567+00:00。数据来自 GitHub Actions 公开 run/jobs REST API；仅只读查询，没有重跑、取消或修改 workflow。详细步骤状态保存在 [remote-ci-status.json](remote-ci-status.json)。

| 提交 | 运行 | Run 状态/结论 | Job | Job 状态/结论 | 当前步骤或失败步骤 |
|---|---|---|---|---|---|
| `1c0c9ea` | [37778455558](https://github.com/justicerains1/AUTH_RUST/actions/runs/37778455558) | in_progress / 尚无结论 | check-and-build (ubuntu-24.04) | completed / success | 无正在执行/失败步骤 |
| `1c0c9ea` | [37778455558](https://github.com/justicerains1/AUTH_RUST/actions/runs/37778455558) | in_progress / 尚无结论 | integration-t01 | in_progress / 尚无结论 | Run npm run test:e2e -- --task=T17 |
| `1c0c9ea` | [37778455558](https://github.com/justicerains1/AUTH_RUST/actions/runs/37778455558) | in_progress / 尚无结论 | check-and-build (windows-2025) | completed / success | 无正在执行/失败步骤 |
| `f5e7640` | [37780190542](https://github.com/justicerains1/AUTH_RUST/actions/runs/37780190542) | in_progress / 尚无结论 | check-and-build (windows-2025) | in_progress / 尚无结论 | Run node scripts/ci-command.mjs check |
| `f5e7640` | [37780190542](https://github.com/justicerains1/AUTH_RUST/actions/runs/37780190542) | in_progress / 尚无结论 | check-and-build (ubuntu-24.04) | in_progress / 尚无结论 | Run node scripts/ci-command.mjs test:unit |
| `f5e7640` | [37780190542](https://github.com/justicerains1/AUTH_RUST/actions/runs/37780190542) | in_progress / 尚无结论 | integration-t01 | in_progress / 尚无结论 | Run node scripts/ci-dev-up.mjs |
| `0f190e6` | [37780426239](https://github.com/justicerains1/AUTH_RUST/actions/runs/37780426239) | in_progress / 尚无结论 | integration-t01 | in_progress / 尚无结论 | Run node scripts/ci-dev-up.mjs |
| `0f190e6` | [37780426239](https://github.com/justicerains1/AUTH_RUST/actions/runs/37780426239) | in_progress / 尚无结论 | check-and-build (windows-2025) | in_progress / 尚无结论 | Run node scripts/ci-command.mjs check |
| `0f190e6` | [37780426239](https://github.com/justicerains1/AUTH_RUST/actions/runs/37780426239) | in_progress / 尚无结论 | check-and-build (ubuntu-24.04) | in_progress / 尚无结论 | Run node scripts/ci-command.mjs check |

表中的进行中状态未标记通过。最终结论须另行查询 completed/conclusion，不能以已完成的部分步骤推定整个 workflow 或系统验收通过。当前状态是一次观察快照，后续运行状态可能变化。

公开元数据可读取，但仅有步骤状态无法证明失败根因。此前 job 日志下载 API 返回 HTTP403“Must have admin rights to Repository”；本次没有尝试读取需要管理员权限的日志，也未读取本地身份秘密。历史失败运行/诊断保留，不以本快照覆盖。远端 CI 自动检查仍不替代实体设备、标准性能环境和生产发布关卡。
