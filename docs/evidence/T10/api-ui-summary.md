# T10 授权入口与同意页面模块记录

日期：2026-10-08（Asia/Shanghai）。本模块实现OAuth authorize HTTP、浏览器绑定事务读取/决定，以及最小真实同意页面。客户端仓储、授权事务/同意/代码提交由独立数据库模块负责；Token交换与最终OIDC元数据仍属后续T11/T12。

## 实际实现

- GET/POST form `/oauth/authorize` 使用同一严格AuthorizationRequest解析，拒重复关键参数、PKCE降级、非法scope/state/nonce/prompt/max_age。协议请求体64KiB，Content-Type唯一且为form；错误不会套中文JSON API envelope。
- 未注册/不精确匹配回调错误留身份域安全HTML，不返回Location。解析失败但唯一client/redirect/state已通过数据库精确注册校验时，才返回标准OAuth error与原state到该回调。数据库失败传播503，不能当false新建不正确上下文。
- Top-level授权只创建短期PG权威预认证/授权事务；保持原有效Cookie绑定，缺失才创建随机摘要上下文。NeedLogin仅回 `/login?transaction=<服务端UUID>`，Consent回内部同意路由；已批准同客户端全部scope时才由数据库安全流程返回代码。
- prompt=none按真实身份/同意返回标准login_required或consent_required；prompt=login/max_age由数据库auth_time判断，原会话不能跳过必要重新登录。
- GET `/api/v1/oauth/transactions/{id}`只当前浏览器绑定可读，显示服务端client/scope/期限与强枚举login_required/consent_required。POST decision只接受approve/deny，经Origin/CSRF与数据库有效身份/事务单次规则后返回completed及服务端已验证回调。
- `/oauth/*` middleware、body错误与未知方法/路由使用标准协议错误或authorize安全HTML，普通 `/api/v1/*`保留安全中文错误结构；统一request_id与no-store/CSP/no-referrer保持。

同意页面只读取服务器事务，React自动转义客户端名称和scope说明，不从用户query接受client/redirect权限。授权必须用户明确点击同意或拒绝；失败可恢复且不自动重放POST。登录续接只接受一个合法UUID，密码/MFA/Passkey成功后跳内部同意页，不允许任意return_to。刷新/返回由GET事务恢复服务端状态。

## 已执行本模块检查

- Server编译与Clippy all-targets --locked -D warnings退出0。
- 前端TypeScript strict与ESLint退出0。
- 身份前端9文件、36项Vitest通过；新增3项同意页面行为：服务端名称/scope与XSS转义、不自动决定；拒绝精确提交deny且失败不重放；login_required显示UUID续接并禁批准按钮。API spy fixture只验证前端行为，不代替真实OAuth流程。
- 根检查、unit、build、OpenAPI/tooling由整合模块执行；契约验证不能冒充未运行的端点验收。

真实T10 API/PG/Redis [集成](integration.txt)实际退出0：GET/POST授权、首次批准/明确拒绝、60秒code、前缀/通配/未知回调域内失败、重复参数与PKCE降级、prompt=none标准错误/state、跨浏览器绑定、十并发仅一次消费与停用客户端拒发code。额外合法参数白名单拒绝未支持return_to；prompt=login/max_age与既有同意按服务端状态验证。

[真实E2E](e2e.txt)2例实际通过：经同源Vite协议代理完成授权→登录UUID续接→同意/拒绝→注册回调。代理精确匹配协议端点，保留/oauth/consent前端路由，不开启公共浏览器OAuth/CORS。

首轮非法base64url PKCE fixture被正确拒绝后改为规范32字节编码；测试添加既有同意场景后全用户code总量断言未计额外合法code，改用正确场景数量。此前直连后端造成浏览器同源续接错误，增加明确协议代理并从身份origin进入，修复后两例通过。原失败记录保留，没有放宽参数/回调/授权校验。测试回调code/state与所有令牌仅进程内存检查，不入日志/trace/截图。最终任务状态与推送由根整合核对。
