# 03207ab 远端 CI dev:up 失败只读核查

观测时间：2026-10-08 10:42～10:43 UTC。公开 GitHub REST API 实际读取 [run 37761994466](https://github.com/justicerains1/AUTH_RUST/actions/runs/37761994466)，目标提交 `03207abdfbdd43c29f48171f7f8d08f1aaf9ba6a`，attempt 1，最终 failure。本核查没有执行本地 Docker、故障注入或测试，没有根据时长猜测失败原因。

## 已确定的事实

| 项目 | 公开 API 实际结果 | 证据 |
|---|---|---|
| Ubuntu check/unit/build | `check-and-build (ubuntu-24.04)` 全部成功 | [jobs.json](jobs.json) |
| Windows check/unit/build | `check-and-build (windows-2025)` 全部成功，包括 Windows OpenSSL 设置 | [jobs.json](jobs.json) |
| integration 前置 | checkout、Node、Rust、`npm ci`、`dev:secrets` 均成功 | [jobs.json](jobs.json) |
| 实际失败步骤 | `Run npm run dev:up`，10:13:22～10:13:35 UTC，failure | [jobs.json](jobs.json) |
| 后续真实测试 | T01～T19 对应集成/浏览器步骤均 skipped；Chromium 安装亦 skipped | [jobs.json](jobs.json) |
| 清理 | always 的 `npm run dev:down` 成功 | [jobs.json](jobs.json) |
| 原失败注释 | 仅 `Process completed with exit code 1.`，无 Docker 具体诊断 | [annotations-113260400965.json](annotations-113260400965.json) |
| job/run 日志 | 两个下载端点均 HTTP 403，`Must have admin rights to Repository.` | [log-access.json](log-access.json) |
| check-run 正文 | output title/summary/text 全 null | [log-access.json](log-access.json) |
| 可下载产物 | artifacts `total_count=0` | [artifacts.json](artifacts.json) |

因此当前只能定位到 `dev:up` 步骤，**具体 Docker/Compose/build/服务健康错误仍未知**。不能把可能的镜像下载、构建依赖或健康问题写成已经证实的原因，也不能用 Linux/Windows 静态检查通过替代未运行的真实集成。

## 已确定的诊断污染

integration failure 后 `Publish sanitized T07 browser diagnostics` 步骤成功执行，并发布一条 T07 密码重置页面未跳转的注释。但本次 T07 集成和 E2E 步骤均 skipped，注释包含历史本机 `/root/code/rust/auth_rust` 路径；它不是本次 T07 运行失败。

实际读取目标提交的 [ci-e2e-diagnostics.mjs](../../../../scripts/ci-e2e-diagnostics.mjs)：该脚本读 `docs/evidence/T07/e2e-diagnostics-sanitized.txt`，而 `git ls-tree` 确认此历史文件已经跟踪。workflow 的诊断步骤条件为全局 `failure()`，所以任何更早的失败都可能重新发布 checkout 中旧 T07 报告。应保留历史证据，但不能把它重新标成当前执行。

Node 20 action-runtime 弃用警告出现在三个 job；它是单独 warning，API 没有证明它导致 `dev:up` 失败。

## 下一次 CI 的具体诊断建议

根代理负责 workflow 修改。本次只读核查建议增加固定目标的 `ci-dev-up.mjs`，让下一次真实 CI 把实际 Docker 错误保存为安全注释：

1. 专用 wrapper 只执行 `node scripts/run.mjs dev:up`，用 `runStage` 捕获 stdout/stderr；固定命令、参数数组、无 shell。不把 `dev:secrets` 或任意 CLI 加到现有 read-only wrapper 白名单。
2. 原始输出仅放本次 `.local/ci/<run-id>-<attempt>/dev-up.txt`，目录 0700、文件 0600。公开输出只经过 `sanitizeDiagnostic`，合并 `process.env` 与解析的 `.local/dev.env` 作为已知秘密来源；后者的生成秘密通常不在进程环境，仅用现有 `process.env` 会漏精确替换。
3. sanitizer 继续删除连接 URL、Basic/Bearer、JWT、私钥和长值，并对明确的 password/secret/token/key 字段赋值删除值。不要发布 env/config/inspect 全文、Cookie、SMTP 消息、HTTP body、数据库连接值或 Docker healthcheck 原文。
4. 失败时保留实际 Docker `ERROR`/`failed to solve`/编译或服务错误行及有限末尾，生成 `CI dev:up failed` 注释；百分号/CR/LF 做 GitHub workflow-command 转义。确保内嵌 `::error`/`::add-mask` 等内容作为注释正文数据，不再当新命令。wrapper 必须返回原非零退出码，诊断获取失败也不能把启动失败改成成功。
5. 可补只读 Docker metadata 的字段白名单，如固定 Compose 服务名、State/Health 与 ExitCode；不运行 `config`/完整 `inspect`/原始日志展示，不关闭服务健康检查。诊断无数据则明确“无额外诊断”，不猜。
6. 给 T07 E2E 原步骤固定 id，诊断条件限定 `failure() && steps.t07-e2e.conclusion == 'failure'`；脚本只读本次 `.local` 新报告或核验本次 run/attempt 标记。若未产生本次报告则输出“没有本次诊断”，禁止读取 tracked 历史文件作为 fallback。

新增 wrapper 的 meaningful 验证应覆盖：本地 env 秘密/短字段/私钥/URL/JWT 删除、真实错误文字保留、失败退出码不变，以及 T07 未执行时不发布旧报告。可以用独立假子进程验证诊断行为，不需要启动 Docker 或影响当前 T21 长时负载。

下一次真实 CI 有了安全诊断后再按具体错误修复。当前失败证据完整保留，集成仍未放行。
