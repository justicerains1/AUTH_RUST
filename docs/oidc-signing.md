# T11 OIDC 签名与授权码兑换

核心 [jose.rs](../crates/identity-core/src/jose.rs) 固定jsonwebtoken11.1.0的RS256/aws-lc签名，OpenSSL0.10.81仅解析RSA私钥并导出公钥n/e。不自写RSA、不允许token头选择算法。Config已验证issuer/RP与私钥；Signer生成固定kid/alg/use的公开JWK，并加载仅公钥的旧JWKS，拒重复kid和私钥字段。

ID Token含固定iss、用户UUID sub、目标client_id aud、iat/exp、原认证auth_time、实际amr、sid；授权码兑换带原nonce。最长5分钟并不超过session/grant到期。sign检查时间顺序、算法事实/nonce/issuer；验证只接受可信kid+RS256、公钥、精确issuer/audience、正常exp，额外检查未来iat容差两分钟、auth_time≤iat与有效amr。时间算术使用checked_add，不能被异常大整数触发溢出。

旧公钥兼容窗口至少12小时+2分钟由轮换配置/runbook保留；Signer不从请求header下载任意jku/jwk、不在运行时接受算法降级。普通ID Token始终检查exp，过期退出hint的另外sid/browser/client限制在T12单独实现，不能直接调用普通verify放宽过期。

存储 [tokens.rs](../crates/identity-store/src/tokens.rs) 仅authenticate_client可构造私有ClientIdentity；secret原文用SHA256摘要常量时间比较，当前enabled从权威数据库读取。Basic语法与form百分号解码由HTTP层严格单次解析，secret不入URL或日志。

code兑换先定位user/session/grant，再按user→session→grant→code锁，重验verified/active/credential_version、所有到期/撤销、client启用、code原client/完整redirect/S256 verifier绑定。签名器在同一未提交事务内生成ID Token；签名失败回滚且code未消费。成功才创建access/refresh256位随机摘要和family、消费code、写审计后一次提交。

access最多5分钟，refresh绝对不超过session/grant，客户端仅获得短期内存原文；浏览器不直接保存令牌。userinfo只接受未消费/未撤销/未到期access kind，联合user/session/grant/client实时检查，sub必有，email/profile按scope过滤。refresh令牌不能访问userinfo；依赖查询失败是503而不是假active或用户无效。

T11阶段discovery只公布已实现authorization_code；refresh使用端点与轮换/replay语义在T12完成后才公布refresh_token支持。T11保存refresh摘要用于紧接的T12，不将尚未运行的刷新功能或OpenID认证标为通过。

已实际核心2项测试通过：固定RS256及公钥字段、正确主体/错误audience/算法拒绝、错误issuer/过长到期拒绝、旧公钥兼容及私钥JWKS拒绝。core/store Clippy已退出0，之前needlessborrow失败修复不禁规则。成熟独立OIDC客户端互操作、真实并发code单消费、userinfo scope和PG故障由T11测试执行器提供证据，最终状态以acceptance为准。
