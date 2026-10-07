# T06 密码登录、本人会话 API 与页面记录

日期：2026-10-08（Asia/Shanghai）。本模块负责 server/sessions.rs、共享AppState/安全Cookie辅助、登录/账号/设备会话最小页面与前端行为测试。数据库事务仓储由独立模块负责；本记录不把后续MFA、Passkey或完整产品页面任务标为完成。

## 实际实现

- `POST /api/v1/auth/login/password`：规范邮箱与受限密码输入；Origin/CSRF与Redis账号5/分钟、IP30/分钟；未知邮箱复用预计算dummy Argon2；已知用户密码验证后交仓储行锁重查verified/active/credential_version。
- 普通成功仅返回OpenAPI `authenticated` user/session/csrf_token，随机主会话12小时、CSRF及预认证轮换；主Cookie和预认证Cookie沿已有生产/开发名称与Secure/HttpOnly/SameSite=Lax/Path=/、无Domain。
- 已确认TOTP账号仅返回 `mfa_required` challenge_id、purpose=login、允许方法与5分钟期限；新预认证绑定挑战，清旧主Cookie，不创建普通身份session。前端清CSRF内存，后续第二因素请求会重新获取绑定CSRF；T08验证接口未在本任务伪造完成。
- 旧合法主会话可在预认证缺失时重新登录，仓储仍验证当前主会话；伪造/失效主Cookie不能作为合法回退。重新登录撤销本用户旧登录上下文与派生授权，原有效授权事务在同一提交迁移，不能由未验证return_to决定跳转。
- 旧PHC密码已验证后使用受并发控制的hash_verified重新编码；创建/修改密码规则仍独立执行Password::new，避免旧短密码升级被新创建规则误拒。
- `GET /me` 与 `GET /me/sessions`每次读PG权威用户/session状态；列表固定排序、签名不透明cursor、默认20最大100，拒重复/未知参数及跨用户游标，UA只返回浏览器类别。
- 删除会话、当前/全部退出必须CSRF/Origin且只本人；对应grant/code/token同事务失效。重复已退出请求只有有效预认证分支幂等204，不撤销他人会话。无实际session更新时保留原已有效预认证Cookie，不发送未写数据库的新Cookie。
- 必要存储/密码服务失败返回503；invalid credentials统一401，不回显原密码/SQL/内部诊断。登录后跳 `/me`，不返回未验证外部地址。

页面提供真实密码登录、账号安全摘要、当前/全部设备退出、本人设备列表与单设备撤销；错误清密码留邮箱、限流显示Retry-After、不自动重放POST。MFA只显示有限状态。登录/退出清理身份Query缓存并轮换CSRF，不写密码、token、Cookie或恢复码到browser storage。

## 已执行验证

| 命令 | 结果 |
|---|---|
| `cargo check -p identity-server --locked` | 退出0 |
| `cargo clippy -p identity-server --all-targets --locked -- -D warnings` | 退出0，无关闭规则 |
| `cargo fmt --all -- --check` | 退出0 |
| `npm run check --workspace @identity/identity-web` | 退出0，TypeScript strict |
| `node node_modules/eslint/bin/eslint.js apps/identity-web --max-warnings 0` | 退出0 |
| `npm run test:unit --workspace @identity/identity-web` | 4个文件、22项测试通过 |
| `npm run build --workspace @identity/identity-web` | 退出0，登录/账号/会话按route拆包 |

新增4项前端测试验证登录失败清密码留邮箱、有限MFA清CSRF而不呈现已登录、429等待文案且不自动重放、撤销Dialog取消不调用API/确认只提交所选ID。API spy fixture仅验证前端状态/交互；不代替真实身份后端验收。

[真实集成](integration.txt)已实际退出0：随机新Cookie/CSRF、12小时session、授权事务迁移、未知/错误/未验证/禁用统一401、MFA无普通session、合法main-only轮换/伪造main拒绝、本人/他人撤销、分页/全部与重复退出、持user锁同步禁用竞争，以及显式到期时间。

[真实E2E](e2e.txt)2例实际通过：浏览器密码登录、保护账号页、退出与MFA有限续接；使用真实API/PG/Redis，没有mock认证。最终任务通过与代码推送由根整合核对acceptance状态后执行。
