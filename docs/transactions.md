# T03 仓储、锁顺序与事务边界

对应plan T03.03/.04/.06。迁移定义全部数据库约束，`identity-store::repository`实现参数化查询与事务原语；T03没有实现密码/MFA/JOSE验证、HTTP认证handler或SMTP发送。仓储方法只有数据库条件验证，调用它不能冒充已验证用户证明。

## 权威状态与摘要

`Digest`固定32字节；所有token/code/action/challenge预认证绑定、CSRF、恢复码和client secret摘要列只接受32字节。构造时不自动hash或降低熵，T04的CSPRNG/摘要服务负责生成安全材料。Debug只显示REDACTED；仓储错误为NotFound/Conflict/Replayed/InvalidState/Inactive/Unavailable，SQLx内部错误、连接字符串、SQL或秘密不出错误Display。唯一冲突23505分类Conflict，FK/CHECK/NOT NULL分类InvalidState，依赖故障Unavailable。

`session_authority`每次查询join用户，检查verified、active、会话未撤销/未到期、credential_version相等。`token_authority`每次单SQL join用户、session、grant、client、token，检查全部权威状态与client归属、token未消费/未撤销，输出最短到期，不缓存有效结果。Redis只用于限流/预认证，不能成为授权权威。PG不可用返回Unavailable，不能回落到旧成功。

Clock是可注入同步trait，SystemClock产生UTC；FixedClock持固定OffsetDateTime供纯规则/仓储边界验证。每个操作获取当前时间，过期采用严格expires_at > now，边界相等已失效。数据库测试使用显式created_at、expires_at与FixedClock，不sleep等待动作到期。

## 统一锁顺序

安全变更统一 `user → session → grant → token/action/challenge`。同级多行按稳定UUID升序锁定。`Repository::begin_security(user_id)`锁users行；通用事务允许disabled/unverified用户供管理员撤销与邮箱验证使用。发行session、锁有效session及普通身份操作分别要求verified/active；邮箱确认只设置verified，不启用disabled用户。

SecurityTransaction内部SQLx事务私有；handler无法取得其连接独立commit。中央LockPhase单调前进，不能在Artifact阶段再去锁session/grant。lock_session只能本人有效session，lock_grant只能该用户与锁定session的有效grant且client enabled。多数动作先找非锁定索引归属，再按本顺序重新读取锁定权威状态，不能信任第一次lookup的旧快照。

| 操作 | 同一事务里的顺序 |
|---|---|
| MFA/Passkey登录完成 | 用户锁与verified/active/version检查 → 单次challenge校验/consume → 新session插入 → 最终commit；新session插入不是锁既有session，用户锁串行化该用户登录/撤销 |
| 邮箱确认 | 用户锁（可unverified） → 目的verify/action hash/user/expiry/consumed检查 → consume action → verified更新 → commit；不发行session |
| 密码重置 | 用户锁 → 预先锁所有既有session/grant/token → 消费reset动作 → T07密码/版本更新和撤销 → 通知outbox → commit；不得先consume action再反向锁session |
| 授权码兑换 | 用户锁 → session锁 → grant锁 → code锁，校验原redirect/PKCE challenge/S256/未消费/到期 → consume code → 同事务insert token → commit；T11服务实际验证Basic/PKCE verifier |
| refresh轮换 | 用户 → session → grant → family全部token UUID顺序锁 → refresh归属/kind/expiry/consumed检查 → consume旧refresh → insert新token → commit；family绝对到期不变 |
| refresh重放 | 相同family锁 → 确认实际已消费重放（T11服务） → revoke family → **commit撤销** → 返回invalid_grant；不能因返回错误rollback撤销 |
| 全会话撤销/禁用 | 用户 → 所有session UUID顺序 → grant UUID顺序 → token UUID顺序 → 相同时间戳更新revoked → commit；禁用/密码版本更新与安全审计由后续service加入同事务 |

`consume_challenge`以purpose/user/预认证摘要或已锁session绑定、expires、attempts<5、consumed_at空为条件原子UPDATE。purpose属于服务端枚举；登录用途仅Preauth，重新认证/enrollment/registration仅已锁本人Session。discoverable passkey_login可user_id为空，但T09须先库验证已存credential识别用户后才选择begin_security用户。

邮箱verified更新必须本事务先成功consume verify动作；只consume verify但未应用verified时commit拒绝并rollback。

`consume_login_and_create_session`是验收用完整数据库动作：单次consume和session插入共同提交。登录challenge被消费但尚未创建session时commit会拒绝并rollback；组合动作插入失败也poison事务，调用者即使忽略错误再次commit也不能提交半个成功。普通SQL错误令PG事务失败，不能被误提交。后续服务必须在调用此原语前完成密码/因素证明，并把其他认证状态更新放在同一事务，不能在handler另开一个session事务。

authorization code只能先通过`lock_authorization_code`验证digest、原redirect与已计算PKCE challenge，再用相同IDconsume；裸UUID不能绕过绑定验证。refresh family必须先集中锁定，读取所有token固定family_expires_at，已有family的新token不可改变该值；新的family只有不存在时允许创建，同一次兑换所有新token保持相同family。token发行须此前成功consume已锁code或refresh；每次兑换必须恰好插入一个access和一个refresh，重复kind或少任一kind的commit拒绝。family绝对到期同时不超过grant/session。已消费且仍属于有效本family的refresh返回Replayed；未知、过期、已撤销或错误归属返回Conflict，不能无条件撤销任意授权。Replayed允许在锁定family里撤销并commit，不被失败状态误rollback。

事务commit只有最终动作调用一次；drop或rollback取消未提交更改。没有服务端handler的T03阶段，所有原语调用仅真实数据库测试，不对外发布部分认证端点。

## Outbox租约边界

邮件outbox使用独立领取事务，以next_attempt_at,id固定顺序、`FOR UPDATE SKIP LOCKED`领取最多100行pending；只领取到期且无租约或租约过期的记录。UPDATE设置新随机lease_id/lease_until并递增attempts，commit以后才返回密文参数给Worker。每次claim在仓储内部生成新UUID租约，Lease.owner是该一次租约nonce，worker_id只是调用worker标识。领取持锁期间绝不调用SMTP。

完成须传入该Lease.owner并匹配lease_id、未过期lease_until与pending状态；即使同worker重新领取，旧Lease.owner也不能完成新租约；成功变delivered，清encrypted_params、lease与原始秘密载荷。stale租约/其他worker不能标记完成。T05增加真实SMTP/outbox重试1/5/15/60/180分钟、租约续期策略与失败通知；租约不能保证恰好一次邮件投递，流程动作必须本身单次幂等安全。

## 测试与后续实现边界

T03真实数据库测试必须覆盖迁移重复执行、十并发相同email仅一行、十并发challenge消费+session仅一个成功、邮箱动作仅一成功且不自动登录、消费verify与verified应用闭环、rollback、过期/purpose/绑定错误、session credential_version/revocation即时无效、outbox并行领取与租约身份限制。纯单元测试只覆盖Clock边界、Digest长度/脱敏与错误分类，不以mock数据库通过代替真实事务。

所有测试在白名单identity_test中独立临时schema隔离，拒绝production与非白名单、清理只删除本轮schema。迁移CLI支持显式production部署但必须额外确认，测试种子/故障注入/清理永不用于production。后续T04/T05/T08/T09/T11/T12/T14补齐证明、协议/审计、安全事务与真实服务验收，不能把T03仓储成功称为认证功能已完成。
