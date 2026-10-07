# T02 威胁、实现与验收矩阵

本文定义后续实现必须防御的具体威胁。映射关联 [plan.md](../plan.md)、[acceptance.md](../acceptance.md) 的稳定编号；列出的预期是测试设计，不是已通过运行结果。T02-SPEC-02 可以验收映射完整性，T03～T24 的真实安全案例仍须逐项执行。

## 资产与攻击者

资产为账号密码摘要、TOTP 种子、Passkey 凭证状态、恢复码摘要、身份 session、OAuth code/access/refresh、客户端秘密、私钥/AEAD key、授权与撤销状态、审计、outbox 和备份。最关键的完整性属性是不能未经合法因素取得身份/后台权限，不能在撤销提交后通过新的权威认证检查。

考虑未认证网络攻击者、控制其他用户/客户端的攻击者、恶意网页、泄漏一次凭证后的重放者、伪造反向代理头的攻击者，以及服务故障和受控并发竞争。浏览器所有身份/角色/事务字段不可信；合法管理员也不能绕过强认证、清除他人 MFA 或删除最后可用管理员。数据服务与密钥文件属于受控基础设施，秘密泄漏仍通过最小存储、日志过滤和轮换验收降低影响。

信任边界与状态机见 [architecture.md](architecture.md)。本矩阵不扩大至多租户、SAML/LDAP、社交登录、公共 OAuth 浏览器客户端或业务权限系统。

## 威胁到稳定任务与案例的映射

| ID / 威胁与触发 | 必须实现的防御 | 实现任务 | 必须覆盖的案例 | 跨任务场景 |
|---|---|---|---|---|
| TH01 邮箱枚举：已有/不存在邮箱注册、重发、找回差异 | 相同 202/文案；账号邮件预算仍 202；未知账号预计算 dummy hash，错误统一 | T04/T05/T06/T07 | T04-SEC-01、T05-MAIL-04、T07-PWD-01、T20-AUDIT-01 | E02 |
| TH02 凭证猜测与资源耗尽：并发密码/MFA/WebAuthn请求 | Redis Lua 原子账号 HMAC/IP/挑战预算；429 Retry-After；4 个 Argon2 任务、250ms等待；5次挑战失败作废 | T04/T06/T08/T09 | T04-SEC-01、T04-SEC-02、T04-SEC-03、T08-MFA-02、T20-AUDIT-01 | E03/E05 |
| TH03 CSRF：恶意 origin POST 登录、同意、退出或变更因素 | 精确 Origin、X-CSRF-Token、preauth/session用途绑定；Cookie/CSRF登录轮换；GET不消费安全动作 | T04/T05/T06/T10/T12 | T04-SEC-02、T06-SES-02、T10-AUTHZ-04、T12-REV-04、T20-AUDIT-01 | E04/E24 |
| TH04 XSS/点击劫持：恶意用户名/客户端名/审计文字注入 | React文本转义、不用任意HTML；CSP self且无unsafe-eval/任意内联、frame-ancestors none、no-referrer/no-store | T04/T16/T19/T20/T22 | T19-ADMINUI-03、T20-AUDIT-01、T22-OPS-01 | E17/E22 |
| TH05 会话固定：登录沿用旧Cookie或错绑OAuth事务 | 随机主Cookie/CSRF轮换；事务升级同提交绑定新session；旧流程不可冒用 | T04/T06/T10 | T06-SES-01、T06-SES-02、T10-AUTHZ-04、T20-AUDIT-01 | E04 |
| TH06 IDOR与横向越权：他人session/credential/grant/user_id | 每个API独立查本人归属；管理员后端授权；403/404不回敏感数据 | T06/T09/T12/T14/T18/T19 | T06-SES-03、T09-PK-03、T12-REV-03、T14-ADM-02、T19-ADMINUI-02 | E17/E26 |
| TH07 回调窃取/开放重定向：前缀/通配符/未验证return_to | 预注册完整字符串精确匹配，生产HTTPS；未知回调错误留身份域；外部回跳只用已验证注册目标 | T10/T12/T14/T20 | T10-AUTHZ-02、T12-REV-04、T20-AUDIT-01 | E12/E24 |
| TH08 授权code截获/注入：错verifier/client/redirect、重复关键参数 | PKCE S256强制、43～128 verifier、43 challenge；绑定原client/redirect/scope/session；重复参数拒绝 | T10/T11/T20 | T10-AUTHZ-01、T10-AUTHZ-02、T11-OIDC-02、T20-AUDIT-03 | E12/E13 |
| TH09 login CSRF/nonce重放：替换callback state/nonce或跨浏览器txn | state/nonce每事务随机并绑定BFF会话；ID Token nonce精确验证；服务端txn不能浏览器改client/scope | T10/T11/T13/T20 | T10-AUTHZ-04、T11-OIDC-01、T13-BFF-01、T20-AUDIT-03 | E11/E12 |
| TH10 JWT算法/issuer/audience混淆：none、HS/RS替换、另一client ID Token | 固定RS256/kid可信JWKS；iss/aud/sub/exp/iat/nonce/auth_time验证；userinfo sub匹配；成熟外部client互操作 | T11/T13/T20 | T11-OIDC-01、T11-OIDC-03、T11-OIDC-04、T20-AUDIT-03 | E12/E21 |
| TH11 code/action/恢复码并发双消费 | 单事务检查与消费、摘要唯一、权威数据库锁；同步屏障证明指定窗口最多一次成功 | T03/T05/T07/T08/T11/T20 | T03-DB-03、T05-MAIL-01、T07-PWD-02、T08-MFA-03、T11-OIDC-02 | E07/E13/E24 |
| TH12 TOTP同step重放或跳过MFA | 原子last_step与challenge消费；密码仅有限挑战无主Cookie；已有MFA危险操作必须近期强因子 | T06/T08/T18/T20 | T08-MFA-01、T08-MFA-02、T08-MFA-04、T18-ACC-02 | E05/E06/E08 |
| TH13 Passkey无UV、错origin/RP/challenge/signature或cross-account | webauthn-rs完整库验证、固定origin/RP、UV=true、discoverable识别已有用户、credential唯一及数量10 | T09/T20 | T09-PK-01、T09-PK-02、T09-PK-03、T09-PK-04 | E09/E10/E23 |
| TH14 同步Passkey计数器误判或取消无回退 | 库处理signCount与backup事实，不要求一概递增；取消可重试或密码回退；真实设备验收 | T09/T17/T23 | T09-PK-04、T17-PAGE-03、T23-REL-02 | E09/E23 |
| TH15 邮件扫描器提前消费/Referer泄露/动作交叉用途 | token fragment立即清URL；显式POST确认；purpose/user/expiry/consumed绑定；重发作废旧verify | T05/T07/T17 | T05-MAIL-01、T05-MAIL-02、T07-PWD-02、T17-PAGE-03 | E01/E24 |
| TH16 邮箱恢复绕过MFA或直接取得登录 | reset仅更新hash/version并全撤销，保留TOTP/Passkey，无自动session；禁止管理员无条件清MFA | T07/T08/T09/T14 | T07-PWD-01、T07-PWD-03、T07-PWD-04、T08-MFA-04 | E08 |
| TH17 refresh重放/宽限隐藏攻击 | 一次使用轮换；保留consumed摘要；再次提交单独提交整family撤销及审计后invalid_grant | T12/T13/T20 | T12-REV-02、T12-REV-05、T13-BFF-04、T20-AUDIT-01 | E14/E25 |
| TH18 撤销与refresh/login竞争产生新有效凭证 | user→session→grant→token/action统一锁序；登录重验version/status；撤销与派生授权同事务 | T03/T06/T07/T12/T14/T20 | T06-SES-04、T12-REV-05、T14-ADM-04、T20-AUDIT-01 | E16/E25 |
| TH19 缓存/长snapshot/滞后副本延迟撤销 | 每次权威联合检查；无active正缓存；Q晚于COMMIT须见无效；BFF每次保护请求introspect | T12/T13/T20/T24 | T12-REV-01、T13-BFF-02、T20-AUDIT-01、T24-HA-02 | E15/E16/E25 |
| TH20 跨客户端token扫描/撤销 | Basic验证client后检查token归属；introspect其他client仅active=false；revoke不修改其他grant | T12/T20 | T12-REV-03、T20-AUDIT-03 | E26 |
| TH21 RP退出DoS/另一用户sid/恶意回跳 | GET/初始form只展示确认；验签iss/aud/sid与当前会话；POST确认Origin/CSRF；无有效hint不外跳 | T12/T13/T20 | T12-REV-04、T13-BFF-02、T20-AUDIT-03 | E15/E24 |
| TH22 BFF浏览器令牌泄漏与多实例重复refresh | 浏览器仅随机应用Cookie；tokens/secret服务器加密存储；A/B独立空间，共享数据库刷新锁 | T13/T20/T24 | T13-BFF-03、T13-BFF-04、T24-HA-02 | E11/E18/E25 |
| TH23 代理伪造绕过IP限流、Host注入issuer/邮件 | TRUSTED_PROXY_CIDRS白名单；其余来源用连接IP；固定issuer/RP/回调，不从Host构造 | T01/T04/T05/T10/T20 | T01-BOOT-02、T04-SEC-02、T20-AUDIT-01 | E03/E28 |
| TH24 PG/Redis故障认证旁路 | PG状态失败关闭；Redis限流入口503；BFF状态查询失败503而非假登录；恢复后实测 | T04/T12/T13/T20 | T04-SEC-03、T13-BFF-04、T20-AUDIT-02 | E18 |
| TH25 SMTP故障或Worker重复领取丢动作 | 加密outbox同事务；SKIP LOCKED租约、提交后SMTP、按1/5/15/60/180分钟重试，成功清秘密 | T05/T20/T22 | T05-MAIL-03、T20-AUDIT-02、T22-OPS-04 | E19 |
| TH26 密钥/种子/secret/token通过日志、指标、备份泄露 | 日志字段禁用、指标有限白名单、AEAD随机nonce与user/purpose AAD、秘密文件权限、独立备份 | T01/T04/T08/T13/T20/T22 | T01-BOOT-02、T04-SEC-04、T13-BFF-03、T20-AUDIT-01、T22-OPS-02 | E20/E21/E28 |
| TH27 管理员越权/未绑因素访问/末位管理员并发删除 | 后端每handler验证membership与近期强认证；CLI唯一受限初始化；集合数据库锁与末位保护 | T14/T19/T20 | T14-ADM-01、T14-ADM-02、T14-ADM-03、T19-ADMINUI-02 | E17/E27 |
| TH28 审计失败仍提交后台危险操作 | 后台变更与必要审计同事务；写审计失败rollback且固定错误不含SQL | T14/T20 | T14-ADM-04、T20-AUDIT-01 | E16/E27 |
| TH29 SQL注入/N+1/任意排序/生产seed破坏 | 参数化SQL、不透明固定排序cursor、约束/索引、白名单测试库及生产拒绝seed | T03/T14/T19/T21 | T03-DB-01、T03-DB-02、T03-DB-04、T19-ADMINUI-03、T21-PERF-03 | E28 |
| TH30 弱生产配置/开发开关/密钥文件缺失 | 启动拒绝非HTTPSissuer、错RP、弱Cookie、明文SMTP、缺秘密或production seed | T01/T03/T22/T23 | T01-BOOT-02、T03-DB-04、T22-OPS-01、T23-REL-01 | E28 |
| TH31 不可恢复备份/轮换导致TOTP损失/旧hint公钥丢失 | 真实独立恢复含DB与keys；AEAD版本兼容/重加密；新公钥先发布旧保留12h+2m | T22/T23/T24 | T22-OPS-02、T22-OPS-03、T24-HA-03 | E20/E21 |
| TH32 UI误导安全结果、占位成功或无可访问回退 | 完整server状态、错误code/请求ID、避免重试危险POST；键盘/读屏/reduced-motion/真实手机 | T16/T17/T18/T19/T23 | T16-UI-01、T17-PAGE-02、T17-PAGE-03、T18-ACC-02、T23-REL-02 | E22/E23 |
| TH33 ID Token当永久业务授权凭证 | ID Token仅身份声明；BFF每保护请求introspect；认证后应用仍执行自己的业务授权 | T11/T13/T20 | T13-BFF-02、T13-BFF-04、T20-AUDIT-03 | E15/E16/E18 |
| TH34 依赖已知High/Critical漏洞或扫描假成功 | 引入前维护/license/RustSec核查；T20 dependency/secret/ZAP实际扫描分诊修复，高危全复测 | T01/T20/T23 | T20-AUDIT-01、T23-REL-01 | E28 |

## 验收设计与发布阻塞

并发案例必须指定同步屏障/受控事务交错：两个 code 或恢复码消费、refresh/refresh、refresh/logout、login/disable；仅连续调用两次不证明竞争窗口。过期边界用注入时钟/明确数据库 timestamps，测试 `now == expires_at` 时无效；密码哈希与状态复核之间的窗口要能受控进入。

撤销验收用事务提交 C 与后续权威查询 Q 证明 Q>C 时无效，记录时间线及数据库状态。已在 C 前通过检查的业务请求允许结束，不要求撤销远端业务事务；TH19/TH33 的防御仍要求下一次检查失败，不把“执行中允许结束”用作延迟缓存的理由。

SMTP、PG、Redis 故障测试必须真实停止/恢复依赖；可用 mock 验证纯逻辑，但不能代替 T20-AUDIT-02 的故障证据。真实 Passkey与五类浏览器不可用保持阻塞；虚拟认证器不代替设备案例。安全报告 High/Critical 非零、认证关键失败、越权/撤销延迟、必要测试未运行或恢复不可用均阻止第一版发布。

## T02 映射审查范围

T02-SPEC-02 检查本矩阵每一行至少有一个真实存在的实现任务、任务验收编号及 E 场景；核对不存在的编号必须失败。当前映射只声称覆盖威胁设计，不声称任何后续行为已运行通过。接口与状态变更时先同步 OpenAPI、本矩阵和原验收文档；不能删除难以实现的威胁行来释放关卡。
