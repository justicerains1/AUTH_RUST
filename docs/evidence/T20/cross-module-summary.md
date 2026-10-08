# T20 跨模块安全证据汇总

2026-10-08 UTC。本文件把已保存的真实模块报告、当日故障复测和新增跨模块检查映射到 [威胁矩阵](../../threat-model.md)。它不修改 [acceptance.md](../../../acceptance.md) 的案例状态，不代替最终完整回归、残留风险分诊或发布放行。

## 本轮实际运行

[integration.txt](integration.txt) 记录 2026-10-08 09:40:42.770～09:40:50.227 UTC 的真实 PostgreSQL、Redis、Axum HTTP 检查，退出码 0：

- refresh 已完成 Basic 客户端认证后等待真实用户行锁；等待期间提交客户端秘密轮换，旧客户端上下文不能继续签发凭证。
- 已签发且未消费的授权码在用户禁用提交后不能兑换。
- token 请求重复关键字段被拒；攻击者 Origin 不能获得浏览器 CORS 权限。
- RP 退出开始后删除注册回调，最终确认重新检查注册状态，不能外跳或完成撤销。

对应实现为 [t20_security.rs](../../../crates/identity-server/tests/t20_security.rs)，隔离目标为 `identity_test` 随机临时 schema；没有模拟认证成功接口。验证边界见 [cross-module-test-boundaries.md](cross-module-test-boundaries.md)。

另发现真实邮件投递后的跨模块契约问题：Worker 为 `email.delivery_succeeded` 写入 `target_type=outbox`，管理员前端严格 DTO 原未接受 `outbox`，导致含投递记录的审计列表解析失败。已精确补齐 API/DTO 枚举，未改变 Worker 记录或放宽为任意字符串。[admin.test.tsx](../../../apps/identity-web/src/pages/admin/admin.test.tsx) 新增已知投递记录形状的 DTO 单元回归；根代理实际执行该文件四项测试通过。该单元使用明确的合成 DTO，不冒充真实 SMTP→管理员页面联动成功；发布版本真实邮件投递后审计列表的 HTTP/浏览器联动仍须由完整最终回归保存证据。

## 威胁与已执行证据

下表的“证据”指实际模块运行记录及其测试边界。历史模块通过不能自动等同于当前提交完整 T20/T23 回归通过；后置整套运行应另存带时间的汇总，不把未执行项填为通过。

| 威胁 ID | 已执行的行为与证据 | 保留的范围边界 |
|---|---|---|
| TH01 邮箱枚举 | 注册/重发已有与未知邮箱统一 202；登录未知、错误、未验证、禁用统一 401；找回和邮件预算检查。[T04](T04-fault-replay-20261008.txt)、[T05](T05-fault-replay-20261008.txt)、[T06](../T06/test-summary.md)、[T07](../T07/test-summary.md) | 证明响应策略，不证明所有互联网延迟都完全一致。 |
| TH02 猜测与资源耗尽 | Redis Lua 十并发账号预算、伪造 IP 下 429/Retry-After；Argon2 参数与队列边界、五次错误 MFA 挑战作废。[T04](T04-fault-replay-20261008.txt)、[T08](../T08/test-summary.md) | T08 仅定向准备本次随机测试预算键，未重置因素重放状态；容量需 T21 独立测量。 |
| TH03 CSRF | 真实 Origin/CSRF/body 边界、登录 Cookie/CSRF 轮换、GET 邮件/退出不消费；本轮 token 重复字段/CORS 拒绝。[T04](T04-fault-replay-20261008.txt)、[T06](../T06/integration.txt)、[T12](T12-fault-replay-20261008.txt)、[T20](integration.txt) | token 客户端认证仍为 Basic；浏览器 Cookie 不替代客户端认证。 |
| TH04 XSS/点击劫持 | 恶意客户端名称仅文本渲染、无事件图片节点；production dist/身份 API 的响应头有限扫描。[T19](../T19/test-boundaries.md)、[ZAP](zap/scan-1791452463058.json) | 本地 production-style 响应不是正式域名/TLS 部署验证；完整手工 XSS 审计不由有限扫描替代。 |
| TH05 会话固定 | 成功登录创建新 Cookie/CSRF，旧 preauth 拒绝；OAuth 事务同提交迁移并保持原绑定。[T06](../T06/integration.txt)、[T10](../T10/integration.txt) | 不接受任意浏览器事务字段重写绑定。 |
| TH06 横向越权 | 他人会话/Passkey 拒绝；跨客户端 introspect/revoke 隔离；普通用户管理 API 403；UI 撤销另一设备后实际 401。[T06](../T06/integration.txt)、[T09](../T09/test-boundaries.md)、[T12](T12-fault-replay-20261008.txt)、[T14](../T14/integration.txt)、[T18](../T18/test-boundaries.md)、[T19](../T19/test-boundaries.md) | UI 隐藏不作为权限证据，后端请求另行检查。 |
| TH07 开放重定向 | 前缀/通配符/非法回调留身份域；RP 确认重新检查已注册回调。[T10](../T10/integration.txt)、[T12](T12-fault-replay-20261008.txt)、[T20](integration.txt) | 正式生产 HTTPS 回调配置还需 T22/T23 实测。 |
| TH08 code 绑定/注入 | PKCE S256、verifier/client/redirect 负向和重复参数拒绝；已禁用用户的现有 code 不能兑换。[T10](../T10/integration.txt)、[T11](../T11/integration.txt)、[T20](integration.txt) | 实际客户端交换，不用自写解析器替代互操作。 |
| TH09 state/nonce/跨浏览器事务 | 跨浏览器事务不可读/消费；BFF 篡改真实 callback state 不创建会话；成熟客户端验证 nonce。[T10](../T10/integration.txt)、[T11](../T11/interop-summary.md)、[T13](../T13/test-boundaries.md) | 这些协议行为不等于 OpenID 官方认证。 |
| TH10 JWT 算法/声明混淆 | 公开 JWKS 与 RS256；独立 jose 拒 none/HS256、错误 issuer/audience/key 和过期；BFF 校验当前声明。[T11](../T11/interop-summary.md)、[T12](T12-fault-replay-20261008.txt)、[T13](../T13/test-summary.md) | 独立库的负向验证和本系统互操作按实际执行范围报告。 |
| TH11 单次凭证并发消费 | 仓储 challenge/action、重置动作、恢复码、code 的十并发最多一次成功。[T03](../T03/integration.txt)、[T07](../T07/test-summary.md)、[T08](../T08/integration.txt)、[T11](../T11/integration.txt) | 行锁/同步屏障检查指定窗口；不是连续调用两次冒充并发。 |
| TH12 TOTP 重放/MFA 跳过 | 密码后仅有限挑战；同 step 十并发最多一会话， fresh challenge 不能重放；危险变更要近期强因子。[T06](../T06/integration.txt)、[T08](../T08/integration.txt)、[T18](../T18/test-boundaries.md) | 没有手改 last_step、恢复码消费计数或 strong_at 来冒充因素成功。 |
| TH13 Passkey UV/origin/challenge/signature/归属 | CDP 虚拟 CTAP2 真实签名注册/登录；未消费 proof 的 origin/challenge/signature 攻击及有效签名但无 UV 被拒，同挑战合法 UV 随后成功。[T09](../T09/test-boundaries.md) | 实体手机/桌面认证器和 Safari 未执行。 |
| TH14 同步 Passkey/取消回退 | 库策略及真实 WebAuthn Abort 后密码回退。[T09](../T09/test-boundaries.md)、[T17](../T17/test-boundaries.md) | 同步凭证在实体设备上的计数/备份兼容仍待验收。 |
| TH15 邮件动作提前消费/泄露 | fragment 立即清 URL、GET 不消费、显式 POST 单次确认、重发旧链接失效、purpose/expiry 负向。[T05](T05-fault-replay-20261008.txt)、[T07](../T07/test-summary.md)、[T17](../T17/test-boundaries.md) | Mailpit/本地 SMTP 送达不能证明生产发件送达。 |
| TH16 恢复绕过 MFA | 重置更改密码版本、撤销现有会话/授权但保留 TOTP/Passkey、不自动登录；仅密码不能关闭因素。[T07](../T07/test-summary.md)、[T08](../T08/test-summary.md) | 删除身份 Cookie 的响应不等于签发有效身份会话。 |
| TH17 refresh 重放 | 同步双刷新一次轮换，失败重放提交整个家族撤销与审计后返回 invalid_grant；签名失败回滚消费。[T12](T12-fault-replay-20261008.txt) | 没有 grace window 掩盖重放；不能把限流/传输错误当作 invalid_grant 成功证据。 |
| TH18 撤销/refresh/login 竞争 | 实际密码请求阻塞用户锁，禁用先提交不发会话；撤销/refresh 两种提交顺序都检查新 token 无效；本轮客户端秘密轮换和已签发 code 禁用边界。[T06](../T06/integration.txt)、[T12](T12-fault-replay-20261008.txt)、[T14](../T14/integration.txt)、[T20](integration.txt) | 证明撤销提交后的下一次权威检查；不要求撤销已通过检查的远端业务事务。 |
| TH19/TH33 权威状态与 ID Token 用途 | 撤销提交后 introspection 无正缓存延迟；A/B 下次保护请求拒绝，状态故障 503；ID Token 不作为永久业务授权。[T12](T12-fault-replay-20261008.txt)、[T13](../T13/test-boundaries.md) | 本地单机/A2 刷新竞争不等于 T24 的数据层故障切换验收。 |
| TH20 跨客户端 token | B 客户端不能 inspect/revoke A 的 token；未知 token 安全 inactive/idempotent；本轮旧 Basic 上下文在秘密轮换后拒发凭证。[T12](T12-fault-replay-20261008.txt)、[T20](integration.txt) | token 归属由服务器权威检查。 |
| TH21 RP 退出 | GET/初始 form 只建确认，坏 hint 和另一 sid 拒绝，CSRF/当前绑定复核；本轮删除回调后确认拒绝。[T12](T12-fault-replay-20261008.txt)、[T20](integration.txt) | 过期但签名有效的 hint 仅在限定当前会话匹配窗口内使用；普通 ID Token 验证仍拒过期。 |
| TH22 BFF 秘密/共享刷新 | 浏览器 storage/响应无 OAuth token/secret，A/A2 十个真实 HTTP 请求共享行锁只一次 refresh。[T13](../T13/test-boundaries.md) | 令牌仅在服务端加密存储；这不是高可用或容量通过。 |
| TH23 代理伪造 | 非可信 forwarding header 无法绕过 IP 预算；固定 issuer/RP/邮件来源及精确回调。[T04](T04-fault-replay-20261008.txt)、[T05](T05-fault-replay-20261008.txt)、[T10](../T10/integration.txt) | 正式代理/端口配置仍需生产主机复核。 |
| TH24 PG/Redis 故障 | 实际停止容器：preauth/限流/introspection 返回 503、无伪 active；BFF 状态故障也 503。[当日故障时间线](fault-replay-current.txt)、[T13](../T13/test-boundaries.md) | closed-pool 单元边界另标，不替代真实容器停机。 |
| TH25 SMTP/Worker 租约 | 真实 Mailpit 停止后 outbox 保留/retry，恢复后投递；不同 worker/同 worker 过期 lease 不能错误完成。[T05](T05-fault-replay-20261008.txt)、[T03](../T03/test-summary.md) | 重试到期用明确数据库 timestamp 准备；未绕过动作消费/租约验证。 |
| TH26 秘密存储/输出 | AEAD 篡改/AAD/key 负向、浏览器秘密隔离、历史 Gitleaks 实际扫描及精确指纹分诊。[T04](T04-fault-replay-20261008.txt)、[T08](../T08/test-summary.md)、[T13](../T13/test-boundaries.md)、[扫描摘要](dependency-secret-summary.md) | Git 历史扫描不自动证明未提交文件或未来发布制品无秘密；独立受控备份需 T22。 |
| TH27 管理员边界 | 实际双 CLI 初始化一次成功；普通/未绑因素管理员拒绝；最后管理员保护，两合格管理员并发删因素保留一人。[T14](../T14/test-boundaries.md)、[T19](../T19/test-boundaries.md) | 没有用手工身份 fixture 冒充管理员因素验证。 |
| TH28 审计原子性 | 强制审计失败后安全/后台变更整体 rollback，固定错误无 SQL。[T04](T04-fault-replay-20261008.txt)、[T14](../T14/integration.txt) | 审计持久写失败必须使必要安全变更失败。 |
| TH29 SQL/分页/seed | 真实约束与白名单测试库拒绝、十万合法 UUID 用户、cursor 下一页 HTTP 与固定排序索引 EXPLAIN ANALYZE。[T03](../T03/integration.txt)、[T19](../T19/test-boundaries.md) | 单次 SQL 计划耗时不等于容量；热 SQL/队列及完整负载需 T21。 |
| TH30 弱生产配置 | 本地生产弱配置/seed 拒绝、生产制品非 root/read-only/config 验证。[T01](../T01/test-summary.md)、[T03](../T03/integration.txt)、[T22](../T22/artifacts-summary.md) | 未在正式 Linux 生产主机部署或证明真实 TLS/秘密挂载/开放端口合格。 |
| TH31 恢复/密钥轮换 | 已有算法版本兼容与制品/操作稿。[T04](../T04/test-summary.md)、[T22](../T22/artifacts-summary.md) | 独立备份、WAL 时间点恢复、签名/AEAD 实际轮换和 RPO/RTO 尚未真实演练，不能标通过。 |
| TH32 UI 安全与可访问性 | 页面等待真实响应、危险操作认证后仍明确确认；恢复码/secret 关闭移除；20 个实际页面 axe、键盘/reduced-motion 自动检查。[T17](../T17/test-boundaries.md)、[T18](../T18/test-boundaries.md)、[T19](../T19/test-boundaries.md) | 读屏/200% 放大/密码管理器/实体手机和五类浏览器人工完整流程仍待验收。 |
| TH34 扫描真实执行 | 依赖/许可证/来源/npm audit/Git 历史秘密/T20 HTTP/有限 ZAP 六阶段均有真实退出码。[security-checks-2026-10-08T09-40-39-229Z.json](security-checks-2026-10-08T09-40-39-229Z.json) | RustSec 为 `cargo audit --no-fetch` 缓存扫描，许可证来源为 locked/offline；不能声称在线公告刷新或完整安全认证。 |

## 当日真实故障时间线

故障窗口独占，T04→T05→T12 顺序运行；当前报告不是历史报告改名。时间为 UTC，细节见 [fault-replay-current.txt](fault-replay-current.txt) 及各独立副本。

| 记录时间 | 实际动作及结果 | 当前证据 |
|---|---|---|
| 09:16:03.557 | 当日故障复测开始，顺序保留共享依赖窗口。 | [总记录](fault-replay-current.txt) |
| 09:16:03.988～09:16:25.353 | T04 真实停止 Redis/PostgreSQL，限流/preauth 权威持久化返回安全 503，恢复后套件退出 0。 | [T04 副本](T04-fault-replay-20261008.txt) |
| 09:16:25.795～09:16:44.839 | T05 真实停止 Mailpit SMTP，动作/outbox 保留并安排重试；恢复后用明确 due timestamp 验证真实投递，退出 0。 | [T05 副本](T05-fault-replay-20261008.txt) |
| 09:16:45.267～09:17:20.734 | T12 真实停止 Redis/PostgreSQL，introspection 返回 503 且无 fabricated active；同时重跑刷新/撤销竞争及成熟客户端，退出 0。 | [T12 副本](T12-fault-replay-20261008.txt) |
| 09:19:29.915850 | 所有命令完成，最终实际确认 PostgreSQL、Redis、SMTP 都 restored/running/healthy，释放故障窗口。 | [总记录](fault-replay-current.txt) |

以上开始/结束是套件与总体恢复记录；未记录每个容器单独 stop/start 的精确瞬间，不虚构更细时间。T13 的 BFF 故障证据来自其独立实际浏览器/API 运行，按 [T13 边界](../T13/test-boundaries.md) 引用，不声称它在这个三套件窗口再次运行。

## 扫描、互操作与剩余验收

[security-checks-2026-10-08T09-40-39-229Z.json](security-checks-2026-10-08T09-40-39-229Z.json) 六阶段退出码均 0，包括实际缓存 RustSec、locked/offline 第三方许可证和来源、在线 npm audit、完整 Git 历史秘密扫描、本轮跨模块 HTTP，以及本地 production-build ZAP。历史秘密告警的逐条理由见 [secret-triage.md](secret-triage.md)，依赖许可义务与扫描来源见 [dependency-secret-summary.md](dependency-secret-summary.md)。

最新已完成的 [ZAP 报告](zap/scan-1791452463058.json) 为 ZAP 2.17.0：清新 session 后 ZAP 自己请求 `/api/v1/me`，匿名 401、真实登录 Cookie 后 200，两阶段 spider/active 均 complete。该次 High/Critical/Medium/Low 为 0，11 条 Informational；信息提示仍按 [分诊文档](zap/triage.md) 审查，报告退出 0 不自动完成风险批准。

扫描目标只为隔离本机测试站点：production dist 加固定 API 代理。spider `maxChildren=10`，预访问公开页面/metadata 和本人 `/me`、sessions/grants；**主动扫描仅 GET `/api/v1/me`，`recurse=false`，API policy，最长两分钟**。它不证明所有写接口、完整 SPA 认证路径、生产 TLS、全部手工攻击或官方 conformance 测试已覆盖。旧运行器失败、开发站点头部告警与后续复测报告均保留；CSP `form-action` 实际修复后复测，没有静默禁规则。

T11/T12 的 openid-client 6.8.8、jose 6.2.12，以及 T13 的维护中 Rust OIDC SDK 实际执行 discovery/Basic/PKCE/state/nonce/token/refresh 或权威检查；范围分别见 [T11 互操作](../T11/interop-summary.md)、[T12 边界](../T12/test-boundaries.md)、[T13 边界](../T13/test-boundaries.md)。没有运行并通过官方 OpenID conformance certification，不宣称系统已获 OpenID 认证。

最终仍需对发布版本完整运行必要 check/unit/integration/e2e/security/accessibility/build，复核修复后结果及残留风险；实体 Passkey/五类浏览器和 T22 的生产恢复条件不可由本报告替代。完整剩余事项见 [T23 remaining-verification.md](../T23/remaining-verification.md)。
