# 完整测试总结

生成时间：2026-10-08T12:29:06.637Z。
代码版本：add1400da832c66bfe7fed240b6804b49e769333；工作区：包含未提交变更。
环境：linux 6.18.40.1-microsoft-standard-WSL2；Node v22.22.1；16 logical CPU；内存 15.22 GiB。

本报告记录全部命令的实际执行；单项失败后仍运行后续检查。测试产物的原始输出保存在本机受限目录，不提交含凭据的原始诊断。

| 检查 | 实际退出码 | 耗时（秒） | 结果 |
|---|---:|---:|---|
| npm ci | 0 | 4.22 | 通过 |
| verify:docs | 0 | 0.11 | 通过 |
| verify:openapi | 0 | 0.56 | 通过 |
| test:tooling | 0 | 2.80 | 通过 |
| check | 0 | 31.28 | 通过 |
| test:unit | 1 | 24.59 | 失败 |
| test:integration | 1 | 514.10 | 失败 |
| test:e2e | 1 | 284.33 | 失败 |
| test:security | 1 | 24.54 | 失败 |
| test:accessibility | 0 | 8.54 | 通过 |
| build | 0 | 1.64 | 通过 |

## 结论

未通过：test:unit、test:integration、test:e2e、test:security。完整验收未放行，不能将局部通过视为完整系统通过。

本机原始诊断：.local/full-test/2026-10-08T12-14-09-892Z/。详细任务失败原因与后续状态见 acceptance.md 和各任务证据。本报告不修改任务状态。用户要求测试失败时也提交总结，代码推送不代表生产发布。
