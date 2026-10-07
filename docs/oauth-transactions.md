# T10 受管理客户端与授权事务

存储模块 [oauth.rs](../crates/identity-store/src/oauth.rs) 复用core OAuth严格解析值对象，不提供临时跳过同意入口。当前模块只生成绑定授权码与grant，token交换、签名、introspection和刷新仍按T11/T12独立实现。

客户端创建使用256位随机secret，数据库只保存SHA256摘要；原文仅返回创建调用者短期内存。公开client_id独立于数据库UUID。注册登录与退出回调分别存kind，生产HTTPS、开发仅loopback HTTP，无userinfo/fragment/通配符，授权时比较完整原字符串。允许scope有限openid/profile/email且包含openid。

prepare接收已严格parse的请求并再次检查scope/state/nonce/S256合法性；客户端启用、registeredredirect、scope允许集不匹配不会持久化无效事务。回调未验证时错误留身份域，已验证invalid_scope/promptnone错误才创建标准error/state回跳。代码不接受任意return_to。

授权事务五分钟，绑定有效preauthentication_context摘要或当前verified/active/版本一致的session；登录后T06/T08/T09已实现同事务将绑定移到新session。请求client/redirect/scopes/state/nonce/PKCE/prompt/max_age全部服务端固定，view只返回客户端安全名称、请求/已同意scope、到期和login/consent状态。

prompt=login与max_age要求重新认证。迁移 [0005](../migrations/0005_authorization_prompt_reauthentication.sql)保存被强制登录的原auth_time，旧会话不能凭仍然有效跳过重新认证；需新session认证时间大于旧值。prompt=none无登录/需同意返回login_required/consent_required，不显示交互。已有同意只覆盖同user/client已批准scope，新增scope仍需同意。

decision先锁user、本人当前session，再锁事务并复核状态；当前user/session/client/grant状态均不使用active缓存。客户端FOR SHARE锁保持启用至本次code提交；后续客户端停用服务不得持client锁后反向取user锁，统一约定状态修改锁顺序，避免死锁。

允许决定一次创建grant（到期不超过session）、256位code摘要（60秒）及原redirect/PKCE/nonce绑定；consent只并集本客户端scope、事务consume与审计同提交。deny独立方法一次consume、写审计、返回access_denied+原state，不生成code/grant。重复decision、过期事务、跨浏览器ID拒绝；Token原文只作为code回跳短期返回，Debug/日志不输出。

本模块已真实通过store编译和Clippy；HTTP标准错误/浏览器同意页面、真实并发及跨客户端fixture验收由T10测试执行器后续完成，不以数据库编译声明OAuth全系统实现或协议认证。
