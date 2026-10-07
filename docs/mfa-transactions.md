# T08 MFA 与恢复码事务

核心算法使用 [mfa.rs](../crates/identity-core/src/mfa.rs) 的totp-rs封装，固定HMAC-SHA1、6位、30秒、±1步；返回匹配的counter由数据库last_step决定是否可用。种子20字节来自OS随机源，base32仅一次绑定流程响应，Debug不输出；恢复码10个、各16随机字节、22位base64url，数据库只保存32字节摘要。

存储实现 [store/mfa.rs](../crates/identity-store/src/mfa.rs) 所有动作先锁用户，再锁本人会话（登录为预认证绑定）、用途挑战/因素；一次提交验证因素、更新last_step或消费恢复码、消费挑战、创建身份会话或更新近期强认证。用户必须verified/active且challenge的credential_version与当前用户一致。

首次绑定要求显式password_confirmed_at最近5分钟；已有TOTP/Passkey要求strong_at最近5分钟。enrollment将新种子以user/totp-enrollment AAD加密存入五分钟challenge，不覆盖已确认因子；confirm有效码后保存user/totp-seed AEAD密文/kid/12字节nonce，confirmed+last_step+恢复码10个+challenge消费+通知审计同事务。未正确确认不会启用因子。

登录挑战绑定预认证，验证成功后主session/CSRF随机轮换，原授权事务迁移新session、原预认证撤销、新预认证创建同事务。重新认证挑战仅绑定当前本人session，更新strong_at但不改变原auth_time/绝对期限。`reauth_only`由专用HTTP端点设置，存储在任何因素或挑战变化前核对purpose，不能消费登录挑战后再返回重新认证错误。

TOTP同一步或更早步拒绝；恢复码单次原子消费。失败次数通过独立提交保留，5次失败将challenge consumed并清加密待绑定状态；错误响应不能rollback预算。重新开始挑战仍受入口/IP/挑战限流，不永久锁账号。

关闭TOTP和重建恢复码要近期合格认证，通知outbox与审计同提交；重建删除旧码全部摘要再生成新集合，旧码立即无效。当前账号均有密码；关闭TOTP且无Passkey时清恢复码，避免残留恢复码成为未配置因素的隐式登录能力。管理员无条件清MFA与人工恢复接口不提供。

通知Worker白名单新增mfa.totp_enrolled/mfa.totp_removed/mfa.recovery_codes_regenerated，无种子/验证码/恢复码出现在邮件或日志。算法Clock可注入，真实测试与HTTP使用同一FixedClock推进时间步，不写last_step模拟重放成功。
