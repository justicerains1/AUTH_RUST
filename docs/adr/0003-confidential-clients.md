# ADR-0003：仅接入服务端机密 Web/BFF 客户端

- 状态：接受。
- 日期：2026-10-07。
- 关联：T02.01/T02.04、T10～T13/T20。
- 验收：T10-AUTHZ-02、T11-OIDC-01/02/04、T12-REV-03/04、T13-BFF-01/03/04、T20-AUDIT-03；E11/E12/E26。

## 背景与决定

第一版已明确仅支持服务器Web/BFF，不实现公共浏览器OAuth客户端。固定Authorization Code+PKCE S256、refresh_token、client_secret_basic和RS256，scope仅openid/profile/email，回调完整字符串预注册。浏览器只持随机应用session Cookie，access/refresh/client secret留BFF并按需加密；应用A/B独立client、回调、Cookie、会话空间与加密凭证上下文。

RFC9700 §2.1.1推荐机密客户端PKCE；系统将其提高为所有客户端强制，并保留state/nonce每事务绑定，防降级和code注入。§2.4禁止password grant，本系统不将平台自身密码登录JSON API误声明为OAuth password grant。§2.5推荐非对称client认证，本版按已选路线使用Basic和高熵随机secret摘要，后续不能以推荐为由自行扩展private_key_jwt/mTLS。

authorize首先固定验证client/redirect/scope/state/nonce/PKCE/prompt/max_age，再进入预认证/同意事务；GET和POST form共享解析与检查，仅创建短期流程。同意POST绑定Origin/CSRF，原子决定，不能用URL改scope。token form仅Basic，userinfo每次权威状态检查与scope过滤；introspect其他client token只有active=false，revoke不越权修改。

ID Token验证必须固定可信算法/JWKS、iss、aud、exp、nonce和实际auth_time/amr。refresh不刷新原认证时间且不重复nonce；userinfo sub必须与ID Token主体对应。RS256旧公钥保留至少12h+2m兼容退出hint，不放宽正常exp验证。RP退出请求只建立确认流程，确认POST后撤销当前sid；hint验证iss/aud/signature/当前浏览器sid/client，注册外跳完整匹配。

## 规范差异与支持声明

实际读取OIDC Core errata set2 §3.1.2.1发现authorize必须GET+POST；初始计划只有GET，主任务已决定同步为GET/POST form，共享校验而不改变同意CSRF边界。RP-Initiated Logout §2同样要求Logout Endpoint GET+POST；主任务已决定 `/oauth/logout` GET/POST form 共享验证并展示确认，`/oauth/logout/confirm` POST 保持 Origin/CSRF 后撤销。不能以另一个路径的POST替代规范要求并宣称已完成认证。

动态注册/implicit等未纳入本版；discovery仅公布已实际可用能力，不能因为契约列出refresh就提前在实现未完成时声明支持。只有实际外部成熟client互操作和所运行conformance模块能形成对应证据；当前T02规范文档不代表已通过OpenID认证。

## 取舍与验证

BFF多一次状态检查与共享refresh锁，换取撤销和令牌不入浏览器。T13必须运行A→B SSO、首次同意、全部退出、token存储检查、并发刷新及依赖故障。T10/T11/T20验证换client/redirect/verifier/nonce、算法和重复参数负向案例。来源见 [spec-sources.md](../evidence/T02/spec-sources.md)，威胁见 [threat-model.md](../threat-model.md)。
