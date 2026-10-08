# T23 远端 CI 状态观察

实际观察 UTC：2026-10-08T13:46:43.257726+00:00。GitHub Actions 公开 run/jobs REST API 一次查询快照，未重跑、取消或修改任何 workflow。详细步骤在 [remote-ci-status.json](remote-ci-status.json)。[12:57 UTC 原始快照](remote-ci-status-2026-10-08T12-57-17Z.md) 与 [JSON](remote-ci-status-2026-10-08T12-57-17Z.json) 已完整归档，不覆盖历史状态。

| 提交 | 运行 | Run 状态/结论 | Job | Job 状态/结论 | 当前步骤或失败步骤 |
|---|---|---|---|---|---|
| `bc742d1` | [37786403983](https://github.com/justicerains1/AUTH_RUST/actions/runs/37786403983) | in_progress / 尚无结论 | check-and-build (windows-2025) | in_progress / 尚无结论 | Run node scripts/ci-command.mjs check |
| `bc742d1` | [37786403983](https://github.com/justicerains1/AUTH_RUST/actions/runs/37786403983) | in_progress / 尚无结论 | integration-t01 | in_progress / 尚无结论 | Run npm run test:integration -- --task=T03 |
| `bc742d1` | [37786403983](https://github.com/justicerains1/AUTH_RUST/actions/runs/37786403983) | in_progress / 尚无结论 | check-and-build (ubuntu-24.04) | in_progress / 尚无结论 | Run node scripts/ci-command.mjs test:unit |
| `0f190e6` | [37780426239](https://github.com/justicerains1/AUTH_RUST/actions/runs/37780426239) | completed / success | integration-t01 | completed / success | 无正在执行/失败步骤 |
| `0f190e6` | [37780426239](https://github.com/justicerains1/AUTH_RUST/actions/runs/37780426239) | completed / success | check-and-build (windows-2025) | completed / success | 无正在执行/失败步骤 |
| `0f190e6` | [37780426239](https://github.com/justicerains1/AUTH_RUST/actions/runs/37780426239) | completed / success | check-and-build (ubuntu-24.04) | completed / success | 无正在执行/失败步骤 |
| `f5e7640` | [37780190542](https://github.com/justicerains1/AUTH_RUST/actions/runs/37780190542) | completed / success | check-and-build (windows-2025) | completed / success | 无正在执行/失败步骤 |
| `f5e7640` | [37780190542](https://github.com/justicerains1/AUTH_RUST/actions/runs/37780190542) | completed / success | check-and-build (ubuntu-24.04) | completed / success | 无正在执行/失败步骤 |
| `f5e7640` | [37780190542](https://github.com/justicerains1/AUTH_RUST/actions/runs/37780190542) | completed / success | integration-t01 | completed / success | 无正在执行/失败步骤 |
| `1c0c9ea` | [37778455558](https://github.com/justicerains1/AUTH_RUST/actions/runs/37778455558) | completed / success | check-and-build (ubuntu-24.04) | completed / success | 无正在执行/失败步骤 |
| `1c0c9ea` | [37778455558](https://github.com/justicerains1/AUTH_RUST/actions/runs/37778455558) | completed / success | integration-t01 | completed / success | 无正在执行/失败步骤 |
| `1c0c9ea` | [37778455558](https://github.com/justicerains1/AUTH_RUST/actions/runs/37778455558) | completed / success | check-and-build (windows-2025) | completed / success | 无正在执行/失败步骤 |

`1c0c9ea`、`f5e7640`、`0f190e6` 的 run 及全部三类 jobs 均 completed/success，按本次实际公共元数据可记录这些提交的 CI 成功。最新 `bc742d1` 尚在进行，不能据先前提交成功推定该版本 CI 通过；最终结论需要后续独立观察。

此快照只记录远端自动 CI，不替代本地完整测试、实体 Passkey、浏览器人工矩阵、参考性能环境或真实生产发布。公共步骤元数据不足以证明失败根因；本次未读取管理员日志，之前日志下载 API 返回 HTTP403“Must have admin rights to Repository”。没有读取本地身份秘密，历史失败证据继续保留。
