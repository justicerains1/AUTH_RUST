# T07 密码重置、修改与最小页面记录

日期：2026-10-08（Asia/Shanghai）。本模块负责HTTP密码端点、认证提示与密码重置/修改最小真实页面，数据库事务及邮件投递由独立模块实现。所有接口复用已通过的共享PasswordService、Origin/CSRF、请求ID、安全错误与Redis预算。

## 已实现

- `POST /auth/password-reset/request`接收email，规范化后应用邮件IP/账号预算。已存在/未知/账号预算超额统一202；IP超限429/Retry-After。数据库生成15分钟reset动作与加密outbox，链接固定ISSUER `/password-reset#token=...`。
- `POST /auth/password-reset/confirm`接收token/password，严格字段和重复键检查，Password::new完整创建规则、Argon2在事务外；数据库锁定并检查动作目的/归属/到期/未消费，原子更新密码/版本、消费、撤销全部会话与派生授权、审计和安全通知。成功返回 `{status:password_reset,next:login}`，只清主Cookie，不自动登录，保留TOTP与Passkey。
- `POST /me/reauth/password`接收当前password。通过当前session查用户，密码验证后数据库再次检查session/user/version；无因素只更新独立password_confirmed_at，响应reauthenticated/strong=false，原auth_time与12小时绝对到期不变。已有MFA仅返回session绑定有限reauthentication挑战，密码不能更新strong_at；Passkey-only不能返回空methods假MFA成功，返回强认证提示。
- `POST /me/password/change`接收new_password，当前本人有效session，数据库按无MFA近期显式密码确认/有MFA近期strong_at执行严格五分钟窗口检查。缺少时403 `AUTH_REAUTH_REQUIRED`附 `{status:reauth_required,required_strength:password|strong,methods:[实际已配置方法]}`，不放宽权限。成功204清Cookie、所有会话/派生授权失效，通知SMTP失败不会回滚已提交密码安全变化。
- ReauthFailure为强枚举wrapper，保持既有ApiError构造兼容；error code/message/request_id与next结构按OpenAPI，所有内部诊断脱敏。请求密码和动作token短期内存并Zeroizing/Drop清理；不输出日志。

页面 `/password-reset`无token为申请邮件；有fragment时立即history.replaceState清理，token仅组件内存，必须用户点击才能POST。失败清新密码、保留邮箱，成功转/login。`/me/password/change`先当前密码确认，再提交新密码；已有MFA/Passkey强认证不足不解锁按钮，403窗口过期重新锁定。autocomplete区分current-password/new-password，不自动重放危险POST、不使用browser storage保存秘密。

## 实际验证

| 检查 | 结果 |
|---|---|
| Rust server编译与Clippy all-targets `--locked -D warnings` | 退出0 |
| cargo fmt全工作区检查 | 退出0 |
| 身份前端TypeScript strict | 退出0 |
| 身份前端ESLint，0warning | 退出0 |
| 身份前端Vitest | 5文件、26项全部通过 |
| 身份前端production build | 退出0，按路由拆包 |
| T07真实API/PG/Redis/SMTP集成 | 退出0，四必要案例和补充事务负向通过 |
| T07真实浏览器E2E | 2例通过，0失败 |

新增4项前端单元测试验证fragment清理/不自动消费与失败清密码、网络失败保留邮箱且不自动重试、已有MFA密码确认不解锁修改、403过期窗口重新锁定。API spy fixture仅验证客户端行为；不代替真实身份认证。

[真实集成](integration.txt)验证旧密码/全部会话和派生token失效、十并发reset仅一成功、verify跨目的失败、TOTP/Passkey保留、显式近期密码与五分钟边界、已有MFA密码不能绕过强认证、强制审计失败回滚完整动作、邮件申请统一202。[真实E2E](e2e.txt)验证浏览器重置与实际近期重新认证后的密码修改。

首轮测试把清除主Cookie的Set-Cookie误判为签发新会话而失败；改为检查禁止有效主Cookie、仅允许Max-Age=0删除后真实复测通过，原失败记录保留。有限MFA续接只提供安全挑战和提示，T08因素验证不在T07冒充完成。最终任务状态与推送由根整合核对后执行。
