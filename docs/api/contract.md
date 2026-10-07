# T02 平台 JSON 与 OAuth/OIDC 实施契约

本契约配套 [OpenAPI 3.1](openapi.yaml)，对应 plan.md 第 6 节、T02.04/.05；当前仅描述后续 T04～T14 应实现的端点。`x-implementation-status: planned` 不表示接口可用。T01 实际 `/health/live`、`/health/ready` 保持原工程行为。

OpenAPI 使用根 server `/` 与每条完整路径：平台 JSON 统一 `/api/v1`，标准协议位于 `/.well-known/openid-configuration`、`/oauth/*`，避免将协议端点错误拼到 `/api/v1/oauth/token`。精确操作清单见 [API 覆盖记录](../evidence/T02/api-coverage.md)。

## 输入、身份与响应约定

成功响应按端点返回明确 schema；无统一成功 envelope。UUID 服务端生成，时间为 UTC RFC 3339，整数 OAuth 时间为 Unix 秒。所有 JSON 请求只允许 schema 明确字段；禁止重复关键字段、未知字段或多个同名协议参数悄悄覆盖。无 requestBody 的操作不接收非空 body。

JSON 普通请求体 32 KiB，认证/密码/MFA/WebAuthn 为 64 KiB；协议 form 为 64 KiB。超过上限 JSON 返回 413 `REQUEST_TOO_LARGE`，协议返回安全的标准 `invalid_request`。WebAuthn byte 字段为无 padding base64url，必须在传给浏览器前转换为对应 ArrayBuffer；回传必须规范编码，`id` 与 `rawId` 一致，库完整检查 origin、RP、challenge、签名、UV、credential/userHandle 归属。字段结构校验不能代替库验证。默认 ES256 与 RS256 公钥凭证参数只用于 WebAuthn；OIDC ID Token 固定 RS256。WebAuthn attestation=`none`，注册要求 discoverable/UV；无法确证硬件保护时禁止 amr=`hwk`。

邮箱严格 ASCII，trim 后全地址小写，<=254 字符、本地部分<=64；点号和 +tag 保留。密码 15～128 Unicode 字符且 UTF-8 <=512 字节，允许空格粘贴，不强制组合或周期修改；服务端执行本地版本化弱密码检查。示例里的密码、token、TOTP 种子、恢复码、签名都为无效占位；不得把示例作为种子凭证。

生产主 Cookie `__Host-identity`，预认证 `__Host-identity-preauth`：Secure、HttpOnly、Path=/、SameSite=Lax、无 Domain；开发使用 `identity-dev`、`identity-preauth-dev`，仅 localhost 分支允许非 Secure。预认证有效期10分钟，普通 session12小时绝对到期；登录轮换随机 session、CSRF及预认证状态，并在同一事务把当前授权事务绑定到新 session。浏览器不在 localStorage/sessionStorage/IndexedDB 持久化密码、验证码、恢复码、CSRF或OAuth token。BFF 服务端保存 OAuth token，浏览器只持随机 BFF Cookie。

所有 JSON 状态变更同时要求精确 `Origin` 与 `X-CSRF-Token`，token绑定当前预认证或当前身份会话。`GET /api/v1/auth/csrf` 返回 `{csrf_token}`；已存在有效身份会话时对应身份会话，否则建立或更新预认证状态。认证成功的 `{status: authenticated}` 返回轮换后的 csrf_token。OAuth authorize GET/POST 和 RP logout GET/POST 属于规范要求的流程入口，只建立短期事务/确认，不执行同意或退出；最终同意及 `/oauth/logout/confirm` 验证 Origin/CSRF。Basic token/introspection/revoke 与 Bearer userinfo 是服务端协议调用，不能用 Cookie 或 CSRF替代客户端/令牌认证。

所有动态身份/账号/后台/协议响应 `Cache-Control: no-store`。公开 discovery/JWKS 可短缓存300秒，轮换旧公钥至少保留12小时+2分钟。每个请求服务端生成 `X-Request-ID` UUID，JSON失败 body request_id必须相同；不信任客户端传入ID。错误禁止 SQL、堆栈、内部主机、secret、token或完整邮件链接。日志不得输出请求query/body、Authorization、Cookie、OAuth code/state/nonce、密码、TOTP种子或完整WebAuthn证明。

## 认证等级与前端确定分支

`x-auth-level` 给出最低身份条件；`x-auth-policy` 描述条件，服务端每个 handler 独立检查。公开与预认证不能访问本人账号；有效普通身份必须邮箱verified、账号active、session未到期/撤销。权限不能依靠前端隐藏按钮，用户已登录不意味着拥有业务应用权限。

| 等级 | 条件 |
|---|---|
| public | 公开元数据或预认证CSRF创建；没有普通身份权限 |
| preauth | 当前预认证Cookie；MFA/授权事务允许文档定义的purpose=session绑定分支 |
| authenticated | 每次读取有效本人身份/session权威状态 |
| recent-auth | 最近5分钟；无MFA账号可用密码，已有MFA必须近期强认证 |
| strong-auth | 最近5分钟TOTP、UV Passkey或一次性恢复码；密码单独不足 |
| admin | 已验证、active、enabled管理员成员且已绑定因素，每次后台访问最近5分钟强认证 |
| oidc-client | 仅预注册机密BFF `client_secret_basic`；token只能属于该client |

管理员未绑定因素时仅可访问本人受限绑定流程，不能访问任何 `/admin/*`。新增管理员只针对已有verified且具有效因素的用户；禁止邀请链接、最后可用管理员删除/停用、管理员代登录、清空其他人MFA。高风险管理更新、即时撤销与审计同事务；审计失败回滚。

认证响应使用 oneOf/discriminator，而不是自由字符串：

| 响应 | schema 与转换 |
|---|---|
| 密码登录成功 | `Authenticated.status=authenticated`，user/session/csrf齐全；进入账号或原授权事务 |
| 密码登录需MFA | `MfaRequired.status=mfa_required`，challenge_id/purpose=login/methods/expires_at；仅展示第二因素，不产生普通身份Cookie |
| 密码reauth无MFA | `Reauthenticated.status=reauthenticated`，strong=false、reauthenticated_at/valid_until；仅能满足无MFA的近期密码条件 |
| 密码reauth已有MFA | `mfa_required`，purpose=reauthentication；只有当前session可消费，不提升strong_at |
| TOTP/恢复码挑战成功 | purpose=login返回authenticated；purpose=reauthentication返回reauthenticated strong=true，不改变原session.auth_time；服务端保存的purpose决定分支，不由客户端选择 |
| Passkey登录/reauth成功 | 库确认UV=true；登录authenticated，reauth为strong=true；不额外要求TOTP，不凭同步计数器武断拒绝 |
| 高风险认证窗口不足 | HTTP403 `{error:{code:AUTH_REAUTH_REQUIRED,...},next:{status:reauth_required,required_strength:password\|strong,methods:[...]}}`；前端选择允许方法重新认证后重新提交原操作 |
| 管理员强认证不足 | HTTP403 `ADMIN_STRONG_AUTH_REQUIRED` 与相同 next.required_strength=strong；普通非管理员为`ADMIN_FORBIDDEN`，不能通过重新认证获得管理员身份 |
| 无有效session | HTTP401 `AUTH_SESSION_REQUIRED`；清理前端身份缓存并回登录，不把401视为业务权限判断 |

MFA challenge、reauth/Passkey challenge均5分钟、最多5次失败、数据库单次消费，purpose/user/session或preauth绑定。恢复码使用 `/api/v1/auth/mfa/recovery/verify` 共用端点，reauth目的必须来自 `/me/reauth/password` 创建的本人session挑战；不提供无绑定恢复码直验接口。TOTP enrollment确认只接受purpose=totp_enrollment；当前±1时间步，原子last_step拒绝已用/更早步骤。首次绑定可近期密码；新增/删除已有认证器、重建恢复码、密码修改必须近期强认证。

## 统一JSON错误、幂等和分页

普通API错误保持 `{error:{code,message,request_id}}`；需要重新认证时另外带受约束 `next`。客户端按code与强枚举next分支处理，不能解析中文message。每个operation声明 `x-error-codes` HTTP→可出现code集合。

| HTTP | 示例code与行为 |
|---|---|
| 400/422 | INPUT_INVALID；ACTION/CHALLENGE输入无效；保留非秘密输入，清理秘密，提示正确输入 |
| 401 | AUTH_SESSION_REQUIRED、AUTH_INVALID_CREDENTIALS、AUTH_FACTOR_INVALID；未知账号走相同密码hash成本，错误不泄漏SQL/secret |
| 403 | AUTH_CSRF_INVALID、AUTH_ORIGIN_INVALID、AUTH_REAUTH_REQUIRED、ADMIN_FORBIDDEN、ADMIN_STRONG_AUTH_REQUIRED；CSRF/Origin失败不执行业务 |
| 404 | RESOURCE_NOT_FOUND；非本人资源与不存在资源同404 |
| 409 | STATE_CONFLICT、AUTH_CHALLENGE_EXPIRED/CONSUMED、AUTH_ACTION_EXPIRED/CONSUMED、AUTH_FACTOR_REPLAYED、AUTH_LAST_LOGIN_METHOD、ADMIN_LAST_MEMBER；单次操作不重复执行 |
| 413 | REQUEST_TOO_LARGE；未消费任何动作/挑战 |
| 429 | RATE_LIMITED；同时Retry-After秒数，恢复预算后重试，不能永久锁定账号 |
| 503 | DEPENDENCY_UNAVAILABLE；必要PG/Redis不可用失败关闭，不能复用缓存认证成功 |

注册/重发/找回未知或已存在邮箱，甚至目标账号预算超额，一律202同文案；邮件可信IP预算超额才429。不把未注册邮箱与已注册邮箱返回不同结构。登录账号HMAC摘要5次/分钟、可信IP30次/分钟；邮件目标3次/小时、IP20次/小时；MFA/WebAuthn结合IP和单挑战五次预算。其他账号/后台/协议具体预算归T04/T21实测配置，operation的policy名标定预算维度，不能当作已量化性能承诺。限流依赖不可用时认证入口失败关闭。

`x-idempotency.mode=none` 不承诺网络重试返回原秘密；`single-use` 必须事务原子消费挑战/动作/code，重放409或统一无效凭证；`idempotent` 只承诺重复状态副作用不增加。不能为了幂等重放恢复码/client_secret。撤销当前/全部session、指定本人session/grant、管理状态和删除成员可幂等；未知或其他人资源仍404。退出操作身份Cookie/预认证Cookie采用OR绑定：有身份session只能由该session确定当前用户；重复已退出请求仅有效预认证Origin/CSRF可204，不接受用户目标参数，不创建新身份session或撤销其他人会话。

所有列表 `limit` 默认20最大100、`cursor`不透明，响应 `{items,next_cursor}`。固定 created_at DESC,id DESC，管理员成员 created_at DESC,user_id DESC，审计 occurred_at DESC,id DESC。cursor绑定排序、过滤、本人/管理员上下文，不接受任意SQL字段；过滤或绑定失配400。当前不新增未定义用户过滤/排序参数。

Passkey列表为 `GET /api/v1/me/passkeys`，详情与名称/删除为GET/PATCH/DELETE `/api/v1/me/passkeys/{id}`；同步plan明确后的完整路由，不临时改名。列表不泄漏公钥、原始credential_id或内部webauthn-rs状态。最多十个。账号没有修改邮箱接口，契约没有修改邮箱按钮/路由。

## 标准OAuth/OIDC profile

只机密Web/BFF、Authorization Code+PKCE S256、refresh_token、client_secret_basic。无implicit、password grant、client_credentials、public browser client、动态注册、device grant、SAML或业务角色。所有BFF强制PKCE，不以client_secret替代PKCE。

`/oauth/authorize` GET与POST form同参数：client_id、response_type=code、redirect_uri、scope含openid、state、nonce、43字符base64url code_challenge、code_challenge_method=S256。state/nonce16～512 ASCII；verifier43～128 RFC7636允许字符。prompt支持login/consent/none，none不可组合；max_age非负按真实auth_time判断。scope仅openid/profile/email且去重；非法或重复关键参数拒绝。重用既有同意只能覆盖同client已批准scope；用户拒绝回access_denied，prompt none缺登录/同意回login_required/consent_required。5分钟授权事务单次完成，code60秒。

完整回调字符串必须预注册，生产HTTPS、开发loopback HTTP；禁止通配符、前缀匹配、fragment/userinfo和任意return_to。未验证client/redirect时只身份域错误HTML；已经验证才回注册URI携带标准error和原state。OpenAPI302的Location是服务端验证结果，不能用请求Host或用户输入拼接。

`/oauth/token` POST form只两类强discriminated grant。authorization_code绑定client、原redirect、PKCE并单次消费；refresh每次轮换，scope不可扩大，绝对不超过原session12小时。已消费refresh重放必须先独立提交整个family撤销，再invalid_grant，不能rollback撤销或短暂宽限隐藏重放。access不透明256位、最长5分钟且不越过原session绝对到期；expires_in=1～300秒，session已到期拒绝签发；ID Token5分钟固定RS256，iss/sub/aud/exp/iat/nonce/auth_time/amr/sid，refresh新ID Token不回放nonce且不重置auth_time。ID Token不是业务授权凭证。

userinfo仅Bearer access；每次PG核查user/session/grant/client/token，profile scope才有display_name（若有），email scope才有email/email_verified。introspection仅Basic本client，未知/过期/撤销/其他归属统一`{active:false}`；active=true字段明确scope/client_id/sub/exp/iat/token_type。撤销refresh撤销整grant，access至少单token；未知/已撤销/错误归属空200同结果。BFF introspection503拒绝保护业务请求，不用有效结果缓存绕过立即撤销。

协议错误为标准 `{error:"invalid_grant",error_description:"..."}`，ASCII安全描述，禁止普通API中文envelope。Basic失败401带WWW-Authenticate；userinfo invalid_token401或insufficient_scope403带标准Bearer挑战；429带Retry-After和temporarily_unavailable。token未知grant用unsupported_grant_type，introspection/revoke未知token_type_hint按RFC7662/7009忽略；只有实际无法撤销的token类型才用unsupported_token_type。既有业务请求可完成，只有撤销事务提交后开始的下一认证检查必须失效，不声称回滚已开始的应用业务。

`/oauth/logout` GET与POST form只验证hint/iss/aud/当前sid/预注册post_logout URI并展示确认，不能直接撤销；`/oauth/logout/confirm` 用户点击后验证精确Origin/CSRF、confirmation_id与浏览器绑定并单次完成。无有效hint仅本地确认页，不外跳；expired hint仅签名仍有效且当前浏览器sid/client匹配可用于退出，常规ID Token仍检查exp。退出后只注册URI带回state；当前无session不创建session、不影响别人会话。确认取消不撤销。

discovery/jwks schema为规划能力，T11发布时须仅报告实际支持并通过独立客户端互操作验收；不能以本文件或自写验签称已获OpenID认证。来源版本、状态机、威胁与实现案例对应见 [架构文档](../architecture.md)。
