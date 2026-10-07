# T06 密码登录与本人会话事务

数据库接口位于 [sessions.rs](../crates/identity-store/src/sessions.rs)。它只接收已校验密码后的用户版本与随机令牌摘要，密码校验、Cookie与CSRF在服务端边界完成。所有SQL参数化；hash以Zeroizing持有，用户凭证Debug不显示hash。复用现有会话、预认证、挑战与授权事务表，本任务无额外迁移。

## 密码校验后的提交

`credential_by_email`读取规范邮箱对应id/hash/verified/status/credential_version及confirmed TOTP。服务端未知邮箱执行预计算dummy验证；已知用户也先完成真实密码验证，不在数据库事务中做Argon2。

`complete_password_login`先锁user，再复核verified、active、expected_credential_version；验证期间密码改变或管理员禁用则统一InvalidCredentials，不能创建会话。参数重编码升级仅更新password_hash/updated_at，不增credential_version或撤销其他设备，因为没有改变密码。

输入包含已有预认证摘要、可选旧主session摘要、候选新session/CSRF与新预认证摘要。旧主session如存在必须属于当前用户；无有效预认证时只有同用户未撤销、未到期、版本匹配的旧session可作为合法重新登录上下文。Cookie/精确Origin/CSRF已经在服务端验证，任意随机Cookie不构成回退许可。

事务按user→旧session（UUID排序）→grant→token/code→流程记录顺序锁定。先定位当前合法预认证/旧session绑定且未消费、未到期的授权事务ID，撤销旧session和派生grant/token时排除这些准备迁移的事务，避免Cookie轮换丢失原授权流程。旧context的挑战消费、新context生成、流程重绑定和会话/audit一次提交。

- 无confirmed TOTP：创建随机主session摘要与CSRF摘要、pwd amr、12小时绝对到期；合法授权事务改为新session并清preauth_hash。旧预认证撤销、新预认证10分钟创建。
- 已confirmed TOTP：仅创建purpose=login、绑定新预认证、5分钟到期、携带当前credential_version的有限挑战；合法授权事务移到新预认证。当前浏览器旧普通session及派生授权撤销，服务端清主Cookie并只发新预认证；第二因素前没有普通身份会话。

事务审计失败整个动作回滚；存储层返回Authenticated或MfaRequired安全状态，不返回原始session token、Cookie或CSRF。第二因素验证与因素开关业务在T08按已有挑战/版本规则实现。

## me与分页

`me`一次权威查询检查当前user/session版本、verified/active、expiry/revocation，再返回OpenAPI的user/session/security字段。时间使用UTC RFC3339序列化；主体sub=id；不返回密码hash、token摘要、原始UA、IP或秘密。安全信息只计数confirmed TOTP、Passkey/未消费恢复码及enabled管理员成员；无因素的管理员标记binding_only，不能由此获得后台权限。

`session_page`默认limit由HTTP层设20，存储接受1～100；固定created_at DESC/id DESC，不接受动态排序SQL。cursor是有限JSON载荷的base64url与HMAC-SHA256，绑定用户、`me/sessions:created_desc,id_desc`用途、位置及10分钟到期；错误签名、跨用户、到期或结构非法拒绝。列表查询再次联合当前有效session，防止前一次me后撤销仍读其他设备数据。只返回当前用户未撤销未到期且credential_version一致的session。

user_agent由HTTP层识别为浏览器/平台枚举，数据库层拒控制字符/超过256字符；不会将任意认证头或自由输入作为设备说明写日志。

## 当前、全部与指定设备退出

`revoke_session`从当前session摘要定位本人user，user锁后复核会话，再锁当前/目标session与派生grant/token/code。目标不属于本人返回NotFound，不能通过请求目标指定其他用户。撤销目标及派生授权、使session绑定挑战/授权事务无效并审计同提交；撤销自己时返回true供HTTP清Cookie。

`logout`仅当前session决定用户；all=false当前设备，all=true该用户全部设备。全部涉及行按稳定ID先锁session、grant、token/code，然后状态更新并创建新预认证及审计。已无有效主session返回false，不执行其他用户动作，服务端保留已被middleware验证的现有预认证Cookie做幂等204，不发一个未入库的候选Cookie。

所有认证读取不使用active正缓存。撤销COMMIT后发起的权威检查必须失败；COMMIT前已通过的业务请求允许完成，不声称取消远端正在执行的业务事务。

## 实际验证边界

本模块已实际`cargo check -p identity-store --locked`与`cargo clippy -p identity-store --lib --bins --locked -- -D warnings`退出0；cursor单元测试覆盖签名、用户、到期与篡改。首次Clippy发现大型枚举和8参数helper，已用Box<SessionView>/AuditContext修复，不禁用规则。

真实密码HTTP、Cookie/CSRF轮换、MFA无普通session、本人/跨用户撤销、授权事务迁移、派生token失效和用户行锁控制的login/disable竞争由T06集成执行器与E2E测试提供证据；最终状态以 [acceptance.md](../acceptance.md) 的实际记录为准，不以模块编译推定通过。
