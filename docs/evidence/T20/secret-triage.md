# T20.01 秘密扫描误报复核

Gitleaks8.30.1对完整Git历史实际返回7个generic-api-key告警，报告redact100%。逐项读取原commit对应行复核：

- 23f3c8e的passkeys.rs第742/768行是调用keyed_account_digest的用途常量`passkey-cursor-v1`，没有嵌入密钥。
- 7f736ac的T15 sha256.json第36行是视觉tokens.json文件校验和，不能用于认证。
- 7f736ac的OpenAPI第16106/17273/20598/21197行是明确无效ID token/hint示例`RVhBTVBMRQ.SU5WQUxJRA.U0lHTkFUVVJF`，解码为EXAMPLE/INVALID/SIGNATURE，无可用签名或身份。

.gitleaksignore只列准确commit/file/rule/line指纹，不忽略文件、路径或整个规则。现源码/全Git历史仍扫描，新增真实告警会失败。受限原始报告在.local，公开报告不包含Secret/Match。

2026-10-08 首次完整回归新增三条告警，安全阶段实际退出 1，失败报告保留在 `security-checks-2026-10-08T12-28-32-006Z.json`。逐项复核 `add1400` 中账号、密码、综合三份负载报告第 4641 行，都是 `sourceSha256` 内 `crates/identity-server/tests/t21_load.rs` 的文件 SHA-256；与实际源码的 `sha256sum` 相同。它是公开源码完整性摘要，没有认证密钥或可用 token。只新增这三条准确指纹，保留原负载报告与扫描规则；修复后再次执行 Git 历史扫描，完整安全阶段还需最终复跑。
