# T11 授权码、签名与 OIDC 互操作验收

2026-10-08，父提交9d76a11；Linux、Rust1.98、Node22、PG17/Redis7.4。模块通过，本次Git提交记录实际代码版本。

- 固定client_secret_basic，严格单次form解码、重复/非法UTF8拒绝；Cookie和body不能替代Basic，不开放浏览器CORS。
- user→session→grant→code统一锁序，绑定当前权限、client/redirect/S256；十并发授权码兑换仅一个成功。签名成功后才创建256位access/refresh摘要、消费code、写审计并提交；签名失败全部回滚。
- ID Token固定RS256/kid/iss/aud/sub/nonce/auth_time/amr/sid，5分钟内且不超过会话/授权。当前与旧公钥JWKS不含私钥，正常验证必须查exp；过期退出hint在T12独立受限实现。
- userinfo每次查权威状态、稳定sub并按scope过滤，拒绝refresh；PG故障真实503，禁用拒绝。
- openid-client6.8.8实际discovery/Basic/PKCE/nonce交换；独立jose6.2.12使用公开JWKS验证，拒none/HS256、错误issuer/audience/key、过期签名。

最终check、unit、build、文档/OpenAPI、63工具测试和真实integration退出0；前端40测试通过。对应记录见final-check.txt、unit.txt、build.txt、docs.txt、openapi.txt、tooling.txt及integration.txt。局部security使用同一T11真实安全/协议harness，不代表T20全量安全扫描。

初始fmt与needless_borrow失败已修复并保留check*.txt；没有禁用Clippy规则。缓存RustSec扫描319依赖、0漏洞，no-fetch报告commit字段为空；缓存数据库HEAD此前独立核对b0797f54ea5d1d5bc1266bff06e201d1c5e07dca（2026-10-07T14:00:26+02:00），不能标在线更新成功。npm安装audit报告0漏洞。GitHub CI既有Windows单元/T03集成失败未在本模块定位或宣称通过。

T11 discovery只公布当前真实authorization_code，刷新/轮换/撤销/RPlogout由T12完成。生产环境未准备、实体Passkey/Safari待验收，未部署或宣称OpenID认证。来源与许可见jose-source.md、interop-summary.md及[签名文档](../../oidc-signing.md)。
