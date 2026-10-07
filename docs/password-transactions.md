# T07 找回、修改密码与近期认证事务

实现位于 [passwords.rs](../crates/identity-store/src/passwords.rs)。哈希计算与密码验证在服务端事务外进行；数据库接口接收Zeroizing编码hash及期望credential_version，SMTP由outbox提交后异步投递。

## 邮箱重置

请求规范邮箱统一202；存在用户在锁user后作废同用途旧reset动作，生成256位随机token、15分钟action摘要及加密reset_password outbox。未验证/禁用账号可申请重设；完成重置不设置verified、不自动启用账号、不创建身份session。未知账号没有写入账号标識，HTTP及日志不回存在性。

确认先摘要定位user，仅作为外键查找，再锁user→该用户所有session（稳定UUID顺序）→grant→token/code→reset action，复核purpose、未消费与expires_at>now。一次提交更新hash、credential_version加一、消费action、撤销全部session/grant/token、作废所有该用户认证挑战，创建security_notification outbox并写审计。邮箱验证action不能作为reset消费，重复或并发reset最多一次成功；审计/outbox失败全部数据库变化回滚。

TOTP、恢复码集合与Passkey验证记录保持原状；旧凭证版本或旧挑战不得用于密码重置后的登录/刷新。禁用账号仍禁用。安全变化已提交后SMTP失败不会撤回密码变化；邮件状态独立重试。

## 近期密码确认与修改

[0003_password_confirmation.sql](../migrations/0003_password_confirmation.sql)新增sessions.password_confirmed_at，默认NULL。它表示显式 `/me/reauth/password` 的完成时间，不用原login.auth_time替代，原12小时绝对期限不延长。

重新认证先用当前合法session读取凭证，服务端实际验证密码，再锁user/session复核version、verified/active、expiry/revoked。无因素账号写password_confirmed_at并返回reauthenticated_at、valid_until、pwd amr；已有TOTP/恢复码仅创建purpose=reauthentication、绑定当前session的5分钟有限挑战，不更新strong_at。仅Passkey可用的账号返回强认证要求，引导后续Passkey重新认证，不生成空methods的MFA结果或临时TOTP旁路。

修改密码检查当前合法session和期望version；无因素必须password_confirmed_at在最近5分钟，有任何既有因素必须strong_at最近5分钟。窗口到界或未来时间拒绝并返回ReauthRequired；仅密码不能关闭或绕过已有MFA要求。合格后更新hash/version、全session/grant/token撤销、全部用户挑战失效、通知outbox与审计同事务。

所有同user安全写入先取得user行锁，因此修改与退出、登录、禁用、刷新串行核对权威版本。本模块重新认证只锁当前session；change在该user锁内再按稳定UUID锁全部会话，随后grant/token。后续不得引入从session开始再取user的反向锁路径。

## 邮件载荷

ResetMail `{reset_url,expires_at}`，链接固定ISSUER `/password-reset#token=<43位base64url>`，15分钟；SecurityNotificationMail `{event,occurred_at}`，event仅password.changed/password.reset_completed，不含密码/任意外链。两种均AEAD绑定user与用途email-outbox，数据库仅保存密文封装。

Worker使用既有run_once租约接口真实投递两个模板，按原1/5/15/60/180分钟退避，成功清密文；安全通知不携带凭证或二维码。实现与SMTP边界见 [email.md](email.md)。

本模块已通过store/Worker编译和Clippy；真实reset/并发/跨用途/过期/旧凭证/SMTP/rollback等案例由T07集成及E2E证据确认，最终以 [acceptance.md](../acceptance.md) 实际记录为准。
