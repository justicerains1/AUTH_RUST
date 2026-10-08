# T22 本地签名轮换与 BFF 兼容演练

2026-10-08 UTC，源码编辑起点`cfc0c70ffed1b675b386a7add8b4a54a877fa8c3`；新增[真实Rust演练](../../../crates/identity-server/tests/t22_signing.rs)及[独立runner](../../../tests/integration/t22-signing.mjs)。没有改生产API、签名算法或BFF验证逻辑。实际生成两组独立RSA2048私钥、随机AEAD及密码，存本次忽略目录，不覆盖现有密钥；独立identity_test随机schema和随机loopback API端口，不停止开发PG/Redis/SMTP。

实际命令`PATH=/root/.cargo/bin:$PATH node tests/integration/t22-signing.mjs`最终退出0：Rust顶层1通过/0失败/0ignored，7条关键相位断言完整执行。首次通过为14:40:30～14:40:38 UTC，runner变量lint修正后又实际复跑0；带时间成功JSON含准确起止时间、提交及源码SHA256。根随后接入T22 runner，完整回归结论单独记录。

## 真实行为

1. API仅旧公钥，真实密码登录、授权同意、Basic/PKCE交换由生产BFF `Oidc`调用，旧kid签名且access权威active。
2. 先在API公开JWKS加入新公钥，仍使用旧私钥签名；实际HTTP JWKS只含公开字段，保留`public,max-age=300`契约。新的BFF `discover`缓存两公钥，真实交换仍旧kid。
3. 切新私钥kid但保留旧公钥，原session/me200、旧ID Token验签成功；预发布的BFF缓存真实refresh取得新签token且不抓JWKS。只缓存旧key的BFF真实refresh新token时实际请求JWKS一次并成功，计数只采集路径、不记录URL参数或payload。
4. 明确模拟**过早删除旧公钥的错误配置**，尚有效旧ID Token验证失败，不能把它当成功退休步骤。另一个已由真实BFF验证并加密持久化的旧签ID Token会话，在只知新key的BFF中仍可用当前refresh获得新凭证，验证user/sid/auth_time/amr不变并保存新AEAD记录；这不授权未认证的旧ID Token绕过验签。
5. 真实签出的旧hint，使用实际`Signer::verify_logout_hint`显式时间参数：auth_time+43320秒恰好通过，+43321拒绝，错audience/sid拒绝。单独实际旧key签出的已过期ID Token被普通verify拒绝，即使合法hint可用；没有关闭普通exp验证。
6. 回滚旧私钥时继续发布两公钥，新的JWT仍可验，真实BFF refresh恢复旧kid签发且active，授权不丢。
7. 注入API clock到原session auth_time+43200秒，旧/me401、当前尚未消费refresh返回400/invalid_grant且无access字段，不因轮换延长原12小时会话。BFF库验证使用系统时钟，签发相位只秒级推进；没有谎称BFF注入时钟、等待12小时或完整浏览器部署观察。

这是应用真实HTTP/PG/Redis和生产BFF OIDC库的本地演练，不是两独立Signer的纯单元验证，也没有伪造成功token响应。私钥、密码、Cookie和JWT不进入PASS/JSON；原始诊断临时0600，仅失败时精确脱敏后保留，runner结束清自身私有目录。没有生成生产recipient、签名配置或发布registry。

## 失败及修复

初轮编译失败101：新测试误用Zeroizing直接SQLx bind/CreatedClient字段名；改为as_str/client_secret并去unused import。第二轮真实启动失败101：IP issuer/RP被WebAuthn库拒绝，改用localhost及随机端口。第三轮授权第二次复用已有同意直接回调导致测试期望consent路径失败，测试明确要求prompt=consent，未改服务同意规则。之后Rust全部通过但runner误预期7条而实际6条，整体仍退出1；补有意义的当前refresh在精确session截止点拒绝断言成为第7条，不只是增加输出。

新增refresh请求曾误用未启用的reqwest form API编译失败101，改用现有url form encoder和Content-Type；定向ESLint发现无用初始变量赋值，移为实际作用域const。所有失败以`signing-rotation-failure-*.json/txt`保存，未改为成功或删除。最终定向Clippy `--test t22_signing --locked -- -D warnings`、ESLint及diff-check均0，真实runner复测0。

正式签名先分发公钥/客户端缓存等待、保留旧key至最后签发兼容窗口、实际生产回滚/多实例分发与至少12小时窗口观察仍需独立环境。完整身份数据库恢复、独立加密存储/14天链与RPO/RTO尚未由本演练证明；T22整体保持原生产待验边界。
