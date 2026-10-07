# PostgreSQL 数据基线、迁移与目标保护

本文对应 T03.01/T03.02/T03.05。17 个核心实体定义于 [0001_identity.sql](../migrations/0001_identity.sql)，仓储锁与消费规则见 [transactions.md](transactions.md)。SQLx 使用固定迁移历史与校验值，重复运行已应用迁移不会重复建表；没有破坏性 down 文件。

本文不声称认证API、SMTP、MFA或OIDC业务已实现。T03真实数据库约束/竞争/rollback证据单独填写；T04仍须实现邮箱完整校验、密码规则、摘要/加密与安全中间件。

## 数据类型与共同约束

主键 UUID 由服务端产生；客户端提供的ID只用于查找并核对归属，不能覆盖用户主体。token、code、CSRF、预认证绑定和恢复码摘要统一 BYTEA 且32字节，原始秘密不入这些表。密码使用不可逆编码hash TEXT，T04验证Argon2格式与资源参数；表的非空/长度约束不替代哈希验证。

全部时间为 TIMESTAMPTZ，迁移连接指定UTC；时间有效性由显式 expires_at/consumed_at/revoked_at表示，`now == expires_at`已失效。测试用明确created/expires字段形成边界，不通过长sleep等待。邮箱数据库CHECK保证ASCII、trim/lower及总长/本地长与基本域格式；完整邮箱语法、DNS标签长度、弱密码和显示名称控制字符策略属于T04服务层。

引用默认RESTRICT/NO ACTION；本版不开放自动销号，不能ON DELETE CASCADE把审计、凭证或撤销摘要当作清理捷径。sessions的(id,user_id)唯一键与grant/challenge组合FK防止会话绑定到另一用户。数组scope固定openid/profile/email；grant、授权事务强制openid；amr仅pwd/otp/rcv/user/hwk，与T02契约一致。

## 17个实体及字段

| 表 | 核心列与约束 |
|---|---|
| users | id、email UNIQUE/规范CHECK、password_hash、display_name、verified、status active/disabled、credential_version BIGINT≥1、created_at/updated_at |
| sessions | id、token_hash32 UNIQUE、user_id、amr TEXT[]、auth_time、strong_at、csrf_hash32、credential_version、user_agent脱敏≤256、expires_at/revoked_at、created_at；有效期≤auth_time+12h |
| oauth_clients | id、公开client_id TEXT UNIQUE、secret_hash32、name、enabled、allowed_scopes、created_at/updated_at |
| oauth_redirect_uris | id、client_id UUID FK oauth_clients.id、uri、kind login/logout；(client_id,uri,kind) UNIQUE；拒空/控制字符/空白/通配符 |
| oauth_grants | id、user_id/client_id/session_id、scopes、expires_at/revoked_at、created_at；(session_id,user_id)组合FK |
| authorization_transactions | id、client_id、preauth_hash32或session_id恰一、固定redirect_uri/scopes/state/nonce/S256、prompt数组/max_age、expires_at≤5m、consumed_at/created_at |
| authorization_codes | id、code_hash32 UNIQUE、grant_id、redirect_uri、S256 challenge/method、nonce、expires_at≤60s、consumed_at/created_at |
| oauth_tokens | id、token_hash32 UNIQUE、kind access/refresh、grant_id、family_id、family_expires_at、expires_at/consumed_at/revoked_at/created_at；access≤5m且不可consumed，refresh轮换保留消费摘要 |
| user_consents | id、user_id/client_id、scopes、created_at/updated_at；(user_id,client_id) UNIQUE |
| email_actions | id、user_id、token_hash32 UNIQUE、purpose verify/reset、expires_at（verify≤30m/reset≤15m）、consumed_at/created_at |
| authentication_challenges | id、user_id可空、preauth_hash32或session_id恰一、purpose、state_encrypted JSONB/state_data JSONB、attempts0～5、expires_at≤5m、consumed_at/created_at；login/passkey_login预认证，其余绑定本人session；仅discoverable passkey_login可user为空 |
| totp_factors | id、user_id UNIQUE、encrypted_seed、encryption_kid、encryption_nonce12、confirmed、last_step≥-1、enrollment_expires_at/created_at；未confirmed须有限期 |
| recovery_codes | id、user_id、code_hash32、consumed_at/created_at；(user_id,code_hash) UNIQUE |
| webauthn_credentials | id、user_id、credential_id BYTEA UNIQUE、库credential_data JSONB、name、created_at/updated_at；用户数量≤10由锁user的service事务保证 |
| admin_memberships | id、user_id UNIQUE、enabled、created_at；最后管理员/合格因素由T14事务保证 |
| email_outbox | id、user_id可空、recipient、template、encrypted_params、state pending/delivered/failed、attempts、next_attempt_at、lease_id/lease_until、delivered_at/failed_at/created_at |
| audit_events | id、event固定词典、actor_id可空FK、target_type/target_id、result success/denied/failure、reason_code、request_id UUID、source脱敏、occurred_at；不存秘密或自由JSON请求体 |

公开client_id和数据库client_id区别：oauth_clients.client_id是协议使用的TEXT标識；所有引用表的client_id是oauth_clients.id UUID。family_id是同grant令牌家族UUID，不新增第18个family实体；同家族grant/绝对到期一致由统一仓储/服务层事务保持。

加密状态/outbox采用JSONB封装 `{kid,nonce,ciphertext}`，三个值均字符串；数据库CHECK要求对象与字段类型，T04的AEAD服务负责base64格式、独立随机nonce、版本key及user/purpose AAD，不在SQL模拟加密。TOTP单独存密文、kid与12字节nonce。Passkey凭证只存库序列化的公共验证状态；待注册challenge秘密进入state_encrypted，state_data仅非秘密对象。

Outbox pending必须含加密参数，lease_id/lease_until必须同时空或同时非空；delivered必须有delivered_at、清encrypted_params、清lease；failed有failed_at及清lease。T05在SMTP成功后一个状态更新清除秘密参数。template固定verify_email/reset_password/security_notification，业务不能注入任意HTML模板或收件链接。

## 索引和清理边界

唯一索引覆盖规范email、session hash、code/token hash、credential ID、成员与TOTP用户、consent/user-client、恢复码/user-hash。附加索引覆盖active sessions/user+expiry、grant/session/user/client、code/grant、token/family/grant、challenge/user/session/expiry、action/user/purpose、outbox到期领取/租约/保留期、审计time+ID及actor/target分页。

服务端使用固定排序的不透明cursor，不能把用户排序字段拼入SQL。T21以EXPLAIN ANALYZE和10万行真实数据检查热查询；索引存在不代表性能已通过。

清理计划：挑战与已消费邮件动作每小时分批，过期session/token每晚分批；已消费refresh摘要保留到family_expires_at+24h以检测重放。审计180天、outbox成功30天、失败90天。父记录存在子FK时先按明确保留策略清子记录再清父记录，不能提前删除refresh重放摘要；无大表长事务扫描。T03提供索引，实际worker调度与边界按后续任务实现。

目前仍为17个计划实体；预认证服务端权威存储方案在T04明确，authorization_transactions/authentication_challenges已含32字节preauth绑定摘要，不擅自新增第18张表。

## 显式迁移目标

[migrations.rs](../crates/identity-store/src/migrations.rs)暴露 `MigrationTarget::from_environment(app_env,database_url,allow_production,test_target)`；只接受development/test/production，DATABASE_URL必须显式PostgreSQL URL，不借PGDATABASE等隐式选择目标。错误只报告固定字段/原因，不含连接密码、SQL或内部host；Debug同样不显示连接参数。

目标依据SQLx解析后的实际database名称判断，不能用URL表面的`/identity_test`掩盖`?dbname=identity_production`。未知query参数会被SQLx日志打印，因此迁移校验在传给SQLx前拒绝未知参数和任意options/search_path；支持明确列出的TLS/连接选项。数据库名限制为63字节ASCII字母数字/下划线便于安全输出。

| 环境/请求 | 放行规则 |
|---|---|
| development | 显式合法URL，打印环境与数据库名后迁移；不隐式创建/删除数据库 |
| test | 实际数据库必须identity_test，即使没有--test-target也不能选别库 |
| --test-target | 必须APP_ENV=test且数据库identity_test；生产即使--allow-production也拒绝测试 |
| production | 合法目标并且显式--allow-production才允许正规迁移；此许可不适用于seed或测试清理 |

运行入口为 [identity-migrate.rs](../crates/identity-store/src/bin/identity-migrate.rs)，示例：

```text
APP_ENV=development DATABASE_URL=<显式且受控的URL> cargo run --locked -p identity-store --bin identity-migrate
APP_ENV=production DATABASE_URL=<受控的生产URL> cargo run --locked -p identity-store --bin identity-migrate -- --allow-production
APP_ENV=test DATABASE_URL=<identity_test的受控URL> cargo run --locked -p identity-store --bin identity-migrate -- --test-target
```

实际秘密不要放shell命令或历史；从受控环境/秘密管理器注入，根Node脚本不回显。命令只示意参数，`<...>`不是可执行值。生产发布须按T22先成功备份、迁移成功、启动/readiness/冒烟；迁移失败非零立即停止发布，不自动降库。

## 测试隔离与实际验证

真实数据库测试限定identity_test，由 `test_schema_options("identity_test_<generated>")` 返回安全search_path/UTC连接选项。schema只接受固定前缀和小写字母/数字/下划线，长度≤63；先验证目标，再创建独立schema，测试结束只清该已验证schema。migration历史表也在该schema；production和development的schema/数据不能被测试创建、删除或seed。

本模块已实际执行：`cargo check -p identity-store --locked`退出0；`cargo test -p identity-store --lib migrations::tests --locked` 4项通过，覆盖显式production许可、production测试拒绝、有效dbname覆盖、未知/options query拒绝、测试schema校验及错误/Debug秘密脱敏。SQL迁移两次、17表约束索引、真实并发和rollback由T03集成执行器验证并记录；仅编译不能将这些案例标为通过。
