# T20.01 秘密扫描误报复核

Gitleaks8.30.1对完整Git历史实际返回7个generic-api-key告警，报告redact100%。逐项读取原commit对应行复核：

- 23f3c8e的passkeys.rs第742/768行是调用keyed_account_digest的用途常量`passkey-cursor-v1`，没有嵌入密钥。
- 7f736ac的T15 sha256.json第36行是视觉tokens.json文件校验和，不能用于认证。
- 7f736ac的OpenAPI第16106/17273/20598/21197行是明确无效ID token/hint示例`RVhBTVBMRQ.SU5WQUxJRA.U0lHTkFUVVJF`，解码为EXAMPLE/INVALID/SIGNATURE，无可用签名或身份。

.gitleaksignore只列准确commit/file/rule/line指纹，不忽略文件、路径或整个规则。现源码/全Git历史仍扫描，新增真实告警会失败。受限原始报告在.local，公开报告不包含Secret/Match。

2026-10-08 首次完整回归新增三条告警，安全阶段实际退出 1，失败报告保留在 `security-checks-2026-10-08T12-28-32-006Z.json`。逐项复核 `add1400` 中账号、密码、综合三份负载报告第 4641 行，都是 `sourceSha256` 内 `crates/identity-server/tests/t21_load.rs` 的文件 SHA-256；与实际源码的 `sha256sum` 相同。它是公开源码完整性摘要，没有认证密钥或可用 token。只新增这三条准确指纹，保留原负载报告与扫描规则；修复后再次执行 Git 历史扫描，完整安全阶段还需最终复跑。


最终 T23 制品证据提交 `ea0e7d7` 后，Git 历史扫描实际报 50 条 generic-api-key（3 条为公开本地 Docker 构建标签，47 条为 JSON 源码/制品/诊断摘要），退出 1，受限原报告保留。逐项读取命中行，另一 agent 独立核对 210 个 sourceFiles 与 `git show 1c0c9ea:<file>` SHA256 全一致，五个构建日志摘要也全一致；54 制品摘要格式和真实运行报告一致，无秘密。完整 [逐条分诊](artifact-secret-triage.json) 仅保存准确指纹及结论；只将这 50 个 commit/file/rule/line 加入 ignore，不删除校验和证据、不忽略文件或规则。更新后再次扫描完整历史，新增告警仍严格失败。

分诊说明重复引用本地镜像标签的四条历史告警也逐项核对，只补对应历史精确指纹；当前说明避免重复该完整标签，原始构建日志与历史记录保留。

2026-10-09本地补测后完整Git历史新增10条告警，均为T21五份报告的源码tokens.rs SHA-256和API可执行文件SHA-256字段。逐行复核，源码摘要与实际文件一致；仅添加这10个精确commit/file/rule/line指纹，未过滤路径或规则，原始受限报告保留。

追加两份T21实际短测报告的4个源码/可执行文件摘要告警，逐项核对仍为tokens.rs与apiExecutableSha256；只新增精确历史指纹。

共享连接后的容量报告两条告警仍为公开源码tokens.rs及API可执行文件SHA-256，逐行核对，只增加精确历史指纹，不更改检测规则。

最后当前令牌池容量/共享连接综合长测报告4条新增告警仍为源码tokens.rs或apiExecutableSha256，逐行审查后仅保精确历史指纹；性能原始结果保留，新增真实秘密仍失败。

当前制品证据139条告警：三份JSON中132个源码摘要逐项与本地SHA256一致，6个重复制品摘要通过对应本地只读镜像sha256sum核对；另1条是诊断选择器明确无效路径测试。只加精确历史指纹，未过滤文件或规则，原始成功/失败证据保留。

最后焦点镜像/短SQL报告49条告警均为公开完整性摘要；45条源码与实际文件SHA逐项一致，三个镜像制品及一API摘要对应既有真实构建输出/本地只读sha256sum核对。只加精确历史指纹，检测范围不变。
