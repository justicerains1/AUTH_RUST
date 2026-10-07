# T02 规范实际来源与关键内容阅读记录

本记录对应 T02.01。2026-10-07 实际使用 Python 标准库 urllib 请求一手规范，读取响应全部字节，计算 SHA-256，并将 HTML 文本解析后阅读以下涉及本版范围的关键章节。所有 9 次请求 HTTP 200；没有将网络请求成功等同于系统实现成功。

SHA-256 对原始响应字节计算，不是浏览器截图或解析后的文本；当前不用巨大全文进入仓库。临时下载仅用于这次阅读，摘要、章节和短摘录保存在本记录。URL 后续内容可能变化，因此 WebAuthn 同时核对了固定日期版本；两个响应字节长度和摘要相同。HTTP last-modified 不是产品实现或规范测试时间。

## 实际响应

| 规范版本 | 实际请求与最终 URL | 读取开始 UTC | HTTP | 字节数 | 响应 SHA-256 |
|---|---|---|---|---|---|
| OpenID Connect Core 1.0，errata set 2，2023-12-15 | https://openid.net/specs/openid-connect-core-1_0.html | 2026-10-07T10:41:22.119049+00:00 | 200 | 437508 | 4d016752a645e8a3c259baa4173b5a90cb6c82898ce868fc8444adde262a5aed |
| OpenID Connect Discovery 1.0，errata set 2，2023-12-15 | https://openid.net/specs/openid-connect-discovery-1_0.html | 2026-10-07T10:41:22.119551+00:00 | 200 | 109112 | 0401a07b35de50e914e195aa2c3c64cc4f6542464381c885d24a2623fbf75ec2 |
| RFC 7636，Standards Track，2015-09 | https://www.rfc-editor.org/rfc/rfc7636.txt | 2026-10-07T10:41:22.119889+00:00 | 200 | 39482 | 1972e5d81cbaba7066cfd46374207bc2b4546b085ed5dd9b034e79e023e0ca31 |
| RFC 7009，Standards Track，2013-08 | https://www.rfc-editor.org/rfc/rfc7009.txt | 2026-10-07T10:41:22.120206+00:00 | 200 | 23517 | b2f346d5a87ba9d7f09047901257a65f7ac0b26352095969c25821afab447829 |
| RFC 7662，Standards Track，2015-10 | https://www.rfc-editor.org/rfc/rfc7662.txt | 2026-10-07T10:41:23.761712+00:00 | 200 | 36591 | 2b7d688cb849f093e860557ac97e6cddac2556d69a4386561b22cdf97bf13657 |
| RFC 9700，BCP 240，2025-01 | https://www.rfc-editor.org/rfc/rfc9700.txt | 2026-10-07T10:41:23.763484+00:00 | 200 | 124673 | 9919d061d40a97886ca866b51c69389b6c81c65cd4b979056c14fdbebfcf622a |
| OpenID Connect RP-Initiated Logout 1.0 Final，2022-09-12 | https://openid.net/specs/openid-connect-rpinitiated-1_0.html | 2026-10-07T10:41:24.090745+00:00 | 200 | 47231 | 246432dafaa5556d896fa9b9ae4284b213ff6921b33478ed3b46bd6bd4aab804 |
| WebAuthn Level 3，W3C Recommendation，2026-08-25 | https://www.w3.org/TR/webauthn-3/ | 2026-10-07T10:41:24.141160+00:00 | 200 | 2739242 | 157030c980d44a3ce4b1ec5bcfaa16790c7dfefac20709af6ed2b11c7120970b |
| 上述 WebAuthn 固定版本 URL，内容摘要一致 | https://www.w3.org/TR/2026/REC-webauthn-3-20260825/ | 2026-10-07T10:44:53.051683+00:00 | 200 | 2739242 | 157030c980d44a3ce4b1ec5bcfaa16790c7dfefac20709af6ed2b11c7120970b |

请求与最终 URL 全部相同，无跳转。RFC 采用官方 `.txt` 形式以便精确章节阅读；计划中的无 `.txt` 页面与其相同 RFC 编号。HTML 页面响应 Content-Type 为 text/html；RFC 为 text/plain;charset=utf-8；WebAuthn为 text/html;charset=utf-8。实际 Last-Modified：Core 2023-12-16 05:56:59 GMT、Discovery 2023-12-16 05:57:04 GMT、RP Logout 2022-09-12 19:03:17 GMT、WebAuthn 2026-08-19 15:41:58 GMT；RFC未返回此头。

## 已实际阅读的关键内容

| 来源与章节 | 原文短摘录 | 对本版实现的准确含义 |
|---|---|---|
| Core §3.1.2.1 Authorization Request | “This URI MUST exactly match one of the Redirection URI values” | 回调采用预注册完整字符串简单比较；不接受前缀/通配符；本版生产另要求HTTPS |
| Core §3.1.2.1 | “Authorization Servers MUST support the use of the HTTP GET and POST methods” | 初始计划authorize仅GET存在差异；主任务决定同步GET/POST form共享校验，只创建短期事务；不是开放无CSRF同意POST |
| Core §3.1.2.2/§3.1.2.3/§3.1.2.4 | “MUST NOT interact with the End-User” when prompt=none；“MUST obtain an authorization decision” | none不得显示交互，未登录/待同意返回标准login_required/consent_required；同意必须在授权释放前成立 |
| Core §3.1.3.7 ID Token Validation | “iss ... MUST exactly match”；“aud ... contains its client_id”；“current time MUST be before ... exp” | 固定可信issuer、aud、签名/RS256/kid、exp和nonce验证；不能只解析JSON相信用户声明 |
| Core §5.3.2 UserInfo | “sub Claim ... MUST be verified to exactly match” | BFF使用userinfo时将sub与ID Token主体比较；平台按scope过滤，sub稳定 |
| Core §12.2 Successful Refresh | “auth_time ... original authentication”；“SHOULD NOT have a nonce Claim” | refresh不能将auth_time改成刷新时间；本版不回放原nonce；保持iss/sub/aud |
| Discovery §3 Provider Metadata | “The algorithm RS256 MUST be included” | 固定声明RS256、实际端点及支持能力；不声明尚未实现或超出范围grant/响应类型 |
| Discovery §4.3 Configuration Validation | “issuer value returned MUST be identical to ... Issuer URL” | discovery issuer与请求固定issuer及ID Token iss完全一致；不从浏览器Host推导 |
| RFC7636 §4.1/§4.2 | “code-verifier = 43*128unreserved”；“BASE64URL-ENCODE(SHA256(ASCII(code_verifier)))” | verifier允许A-Z/a-z/0-9/-._~且43～128；S256结果43位base64url；每事务随机且本版不支持plain |
| RFC7636 §4.4/§4.6 | “MUST associate ... code_challenge”；mismatch “invalid_grant” | 兑换必须用原code绑定的challenge与方法核验；不能由token请求覆盖方法或绕过 |
| RFC7009 §2.1/§2.2 | “verifies whether the token was issued to the client”；“HTTP status code 200 ... invalid token” | Basic认证后归属验证，不撤销别的client；合法客户端对未知/已无效token幂等200，不暴露存在性 |
| RFC7009 §2.1 | “The invalidation takes place immediately” | 本版更精确定义COMMIT后新检查无效，无active正缓存；执行中业务请求允许结束 |
| RFC7662 §2.1/§2.2/§2.3 | “MUST ... authorization”；inactive/unauthorized-to-query “active ... false” | introspect端点Basic认证；其他client的token仅active=false，无额外信息；错误client凭据401 |
| RFC7662 §2.2/§4 | cache “at the cost of liveness” | 规范允许取舍，本版明确选择每次权威查询、不缓存有效认证结果；查询故障不能冒充确定inactive |
| RFC9700 §2.1.1 | confidential PKCE “RECOMMENDED”；“transaction-specific and securely bound” | 本版对全部client强制S256，并保留每事务state/nonce/client/浏览器绑定与防降级 |
| RFC9700 §2.4/§2.5 | password grant “MUST NOT be used”；asymmetric authentication “RECOMMENDED” | 本版拒绝OAuth password grant；平台JSON密码登录不是该grant。client_secret_basic是已定范围，非对称认证推荐不能被冒称为本版已实现 |
| RFC9700 §4.14.2 Refresh Rotation | “previous refresh token is invalidated”；“revoke the active refresh token” | 每次轮换、保留关系摘要，旧token重放撤销家族；本版也对机密client采取该防御，无短期重放宽限 |
| RP Logout §2 | “MUST support ... HTTP GET and POST”；“SHOULD accept ID Tokens ... even when ... exp ... passed” | 同一路径logout支持GET/POST form建立确认；过期hint只可在签名与当前sid/client绑定成立时用于退出，不放宽普通ID Token exp |
| RP Logout §3/§4 | redirect “exactly match”；invalid information “MUST NOT be used” | 外跳仅已注册logout URI，验证失败留身份域；无有效hint且不能证明目标时不外跳；当前无session幂等 |
| WebAuthn Level3 §7.1 Registration | “Verify ... C.challenge”；“C.origin”；“rpIdHash”；UV when required | 本版固定origin/RP并强制UV，交库完整验证后存一次性凭证状态；用户输入不绕过库验证 |
| WebAuthn Level3 §7.2 Assertion | verify “sig ... valid signature”；UV；identified account contains credential | discoverable userHandle必须对应已有用户credential，签名/challenge/origin/RP/UV全过才完成session；反复assertion不可消费两次 |
| WebAuthn Level3 §6.1.1/§7.2 | signCount anomaly “signal, but not proof” of clone；race condition example | 同步Passkey计数器不能一概要求递增，按库/规范处理备份与风险事实，不能虚假声明硬件保护 |

这些摘录来自上述实际下载并解析的文档正文，而不是凭记忆补写。未声称逐字阅读所有规范正文、实现implicit/dynamic registration或其他第一版以外能力；关键阅读范围准确限定为表中章节。

## 与计划和架构的同步

此次发现并报告了authorize与RP logout同路径方法支持差异，主任务已决定同步两份原Markdown和OpenAPI为GET/POST form验证并展示/进入确认流程。只有同意决策、退出确认或账号修改的受保护POST执行安全变化；浏览器CSRF边界保留。

计划7.2“锁动作与用户”的语序与5.3统一用户→会话→授权→token/action顺序容易产生实施误解；主任务已决定明确验证邮箱先锁用户再action。架构以统一锁序为准。来源规范不会替代具体产品期限、密码规则、邮箱恢复政策、管理员边界或单机维护范围。

本记录支持T02来源核查；[architecture.md](../../architecture.md)、[threat-model.md](../../threat-model.md)及ADR-0002～0004定义对应状态与防御。没有运行OIDC互操作、conformance、Passkey、认证业务安全或性能测试，不能据此标记后续功能通过或宣称OpenID认证。
