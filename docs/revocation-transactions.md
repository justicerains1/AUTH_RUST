# T12 刷新、撤销与RP退出事务

实现位于 [tokens.rs](../crates/identity-store/src/tokens.rs)，均直接读取PostgreSQL权威user/session/grant/client/token状态，没有active正缓存。ClientIdentity保存认证时secret摘要，code/refresh/revoke在client锁下重新匹配当前摘要及enabled，避免秘密轮换或停用竞争沿用旧client认证。

刷新先定位归属，再user→session→grant→family token稳定UUID加锁；刷新与所有撤销操作在user锁取得后取当前时间，避免等待中旧时间用于撤销刚创建的token。密码变化/普通会话退出同样重取锁内时间与到期边界。旧refresh consumed时先将整family revoked并审计提交，再返回invalid_grant；不随错误rollback。有效refresh一次消费/新access和refresh/签名成功/审计同提交，签名失败保留旧token。scope只能缩小原token scope，不能再扩大；迁移 [0006](../migrations/0006_token_scope_logout_confirmations.sql)为token单独持久化scope，userinfo与introspection按token实际scope过滤。

introspection正确认证但未知、到期、撤销或跨client token仅active=false；active含本client/sub/scope/iat/exp/type。必要数据库故障抛Unavailable，HTTP必须503，不伪false。revoke未知/已无效幂等，refresh撤整个grant与派生tokens，access至少该token。本人grant页面不透明HMACcursor绑定用户/route/排序与期限，limit1～100；单grant撤销锁user、相关session、grant、tokens并审计。

JOSE增加专用verify_logout_hint：只忽略exp，仍固定RS256/可信kid/iss/aud/sub/sid及认证时间≤12h+2m。普通verify仍检查exp。RP退出GET/POST form只创建五分钟rp_logout_confirmations，绑定当前有效session或预认证，存已验证注册退出URI/state；不从请求sid/return_to决定退出对象。

确认POST由HTTP先Origin/CSRF，再事务校验绑定、未消费/到期；只有session绑定确认才能撤该session及派生grant/token。预认证绑定确认不撤后来取得的身份session。client enabled及精确退出URI在确认时重新核对/锁定，错误或无hint仅身份域确认不外跳；cancel只消费流程，不撤销。撤销、confirm消费、新预认证及审计同提交。

core JOSE3项测试实际通过，含过期hint普通verify拒绝、只有准确aud/sid/sub及兼容期限允许。core/store Clippy通过。真实HTTP/PG刷新重放、跨client、两个提交顺序、RP确认及故障由T12执行器验证；一次并发失败与后续复测保留，不能用无证据猜测归因或删除失败。最终放行以acceptance记录为准。
