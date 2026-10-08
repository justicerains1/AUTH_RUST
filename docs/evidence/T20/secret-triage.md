# T20.01 秘密扫描误报复核

Gitleaks8.30.1对完整Git历史实际返回7个generic-api-key告警，报告redact100%。逐项读取原commit对应行复核：

- 23f3c8e的passkeys.rs第742/768行是调用keyed_account_digest的用途常量`passkey-cursor-v1`，没有嵌入密钥。
- 7f736ac的T15 sha256.json第36行是视觉tokens.json文件校验和，不能用于认证。
- 7f736ac的OpenAPI第16106/17273/20598/21197行是明确无效ID token/hint示例`RVhBTVBMRQ.SU5WQUxJRA.U0lHTkFUVVJF`，解码为EXAMPLE/INVALID/SIGNATURE，无可用签名或身份。

.gitleaksignore只列这7个准确commit/file/rule/line指纹，不忽略文件、路径或整个规则。现源码/全Git历史仍扫描，新增真实告警会失败。受限原始报告在.local，公开报告不包含Secret/Match。
