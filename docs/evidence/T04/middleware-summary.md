# T04 浏览器边界、限流和审计模块记录

日期：2026-10-07；Rust1.98.0，Linux。范围T04.04/.05/.06/.07。当前生产API新增的认证基础端点仅 `GET /api/v1/auth/csrf`；未实现密码登录/MFA/邮箱动作成功流程，未生产开放安全测试fixture。

## 实现

- `migrations/0002_preauth.sql`创建正式表preauthentication_contexts：预认证Cookie/CSRF均32字节摘要，绝对10分钟、撤销与清理索引。PG权威读取，Redis不决定最终身份。
- GETCSRF生成256位token；有效身份session仅更新CSRF，检查user verified/active/版本与session期限；有效preauth保持同一cookie/原始绝对到期，仅旋转CSRF摘要，避免破坏已绑定授权流程；缺失/过期建立新preauth。失败不签发身份session。
- 生产Cookie名称__Host-identity/__Host-identity-preauth，Secure/HttpOnly/Path=/、SameSite=Lax、无Domain；本机开发不同名称。安全中间件所有API状态变更精确Origin与当前session/preauth绑定CSRF，重复Cookie/关键header拒绝。
- 服务端生成UUID request_id并同时响应X-Request-ID与safe JSON错误；不回显SQL/driver/parser诊断。动态响应no-store/CSP/no-referrer/nosniff。未知路由和方法均安全envelope。普通JSON32KiB，认证/WebAuthn64KiB；read_json先递归拒重复JSON成员，DTO仍须deny_unknown_fields完整业务schema。
- main使用ConnectInfo实际socketpeer；白名单外完全忽略转发头。可信代理从XFF右侧剥离可信跳，第一不可信来源停止；坏代理头/过长链安全拒绝，不由Host决定issuer。
- Redis单Lua原子更新IP/账号/挑战多个预算；key是purpose分隔HMAC摘要。login/preauth/mfa/mail分别独立，避免1分钟与1小时TTL混用。密码账号5/分钟+IP30/分钟，邮件目标3/小时+IP20/小时，挑战5/5分钟与IP预算。结果分mail_target与IP超额，目标邮件超额供T05统一202、IP429+Retry-After；必要Redis失败503。
- HMAC key由已配置、持久化活跃AEAD key经identity-rate-limit-v1用途派生；多进程同key预算一致，绝不每次随机key。AEAD活跃key切换会产生新预算namespace，后续密钥轮换运维需在限流窗口内协调保留旧预算或接受明确计数迁移，不能悄悄当限流保证不受影响。
- Store审计helper接受caller同一SQLx事务，event/result/target均受枚举约束；沿用T02架构事件词典，source仅HMAC摘要。事务错误阻止安全变更提交；不保存邮件、密码、code/token/CSRF、种子或WebAuthn证明。

## 已执行验证

- `cargo check --workspace`：退出0；Redis Script feature按维护中的redis1.7.1开启，精确锁新增sha1_smol1.0.1。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`：退出0，无规则关闭。
- `cargo test -p identity-server -p identity-store --lib --locked`：退出0；server4项（伪造转发/可信链/重复JSON/重复cookie-header），store6项既有迁移与摘要/错误测试。
- 独立复核coresecurity：进程共享哈希Semaphore/dummy，permit移入spawn_blocking并保留至实际计算完成；250ms仅队列等待，未降低Argon2资源参数。AEAD完整user/purpose/kid长度前缀AAD+随机nonce，重复keyloader与本地弱清单校验由core真实单测覆盖。
- 真实HTTP/PG/Redis集成最新结果见 [integration.txt](integration.txt)：CSRF/Origin/body、安全错误、转发、10并发账号原子预算、独立policy、429/Retry-After、preauth显式时间到期、Redis停止503、PG停止503，实际退出0。

集成的 `/api/v1/security/check`仅测试创建的Router，用于承载中间件验证；200表示安全边界通过，不是密码/Passkey或登录成功。请求体/query sentinel不进入日志；child失败输出脱敏。实际生产router只合并已实施健康与CSRF路由，不能用fixture证明认证业务已经完成。T05后续调用邮件目标超额标识保持统一202；完整认证端点继续按任务验收。
