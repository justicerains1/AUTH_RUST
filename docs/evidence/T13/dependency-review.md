# T13 Rust OIDC 客户端依赖审查

日期：2026-10-08。推荐精确 `openid=0.25.0`，关闭默认feature，启用rustls；MIT或Unlicense，MSRV1.85，当前Rust1.98兼容。官方仓库 <https://github.com/kilork/openid> 未归档，2026-10-04T20:47:33Z更新；crates.io0.25.0同日发布、未yanked。直接JOSE依赖biscuit0.7 MIT使用ring0.17，HTTP reqwest0.13与项目当前版本相同主线。

候选openidconnect4.0.1（MIT、MSRV1.65、官方ramosbugs未归档）类型成熟，但强依赖rsa0.9，没有feature能移除；RustSec RUSTSEC-2023-0071/CVE2023-49092当前无修复版本。BFF仅公钥验签实际不执行受影响私钥操作，但仍引入已知公告依赖且影响工程审计，故本实现选择无需该依赖的稳定openid。openid-client1.0alpha不作为首选。最终锁文件安全结果必须真实cargo audit记录，不以候选元数据代替扫描。

## 源码语义核查

库`Client::discover_with_client`实现discovery issuer精确一致与JWKS读取。其低层discover未公开且默认JWKS限制HTTPS；本BFF用同一受限HTTPclient读取真实metadata，精确核对issuer后构建库Client与受验证JWKS。生产JWKS仅HTTPS；开发只允许固定loopback origin由同一受限HTTP client读取公开JSON并构造SDK JWKS，实际没有调用jwks_insecure，不能任意HTTP或跳转。BFF进一步校验authorization/token/userinfo/introspection/revocation endpoints与JWKS均属于固定配置issuer origin，禁止URL凭据、fragment或任意metadata URL，HTTP client禁redirect并设置连接/总请求超时，防SSRF与秘密转发。

`request_token_pkce`负责标准form、code/verifier/redirect、Basic交换；授权URL使用库Options state/nonce和S256 PKCE。库随机PKCE函数getrandom unwrap可能panic，BFF用core可失败CSPRNG Token生成verifier，再用库PkceSha256::replicate计算S256。state/nonce同样core随机256位；数据库持久绑定flow-cookie与state摘要、加密nonce/verifier，callback先原子消费对应流程。不能在浏览器存储这些值或OAuth令牌。

`decode_token`先选JWKS kid、匹配JWK alg，再用biscuit验证签名；默认支持多算法，因此BFF只保留kty=RSA、use=sig、alg=RS256公开JWK，要求唯一kid、2048位以上modulus与65537 exponent，拒d/p/q/dp/dq/qi/oth/k和外部密钥引用，不接受none/HS256。SDK仅一个key时会忽略header kid，本BFF在SDK验签之前显式要求kid属于可信JWKS。其后`validate_token`负责iss、aud/azp、nonce、exp与可选max_age。库没有独立拒绝未来iat，BFF补 `iat<=now+120s`、auth_time<=iat、exp>iat且生命周期<=300s，并要求UUID sub/sid与必要auth_time/amr；refresh新ID先完成SDK验证，再与此前AEAD可信会话的sub/sid/auth_time/amr比较，禁止原nonce和旧refresh不轮换。该补充在库验签之后读可信claims，不自写签名或取代库验签。

库Basic使用reqwest.basic_auth直接编码原始clientid/secret。演示配置限制clientid为ASCII安全标识、secret为服务端随机base64url，避免保留字符差异；如未来允许任意保留字符，需要规范form编码的专用transport同步接口，不能悄悄改client_secret_post。SDK Client/Bearer自带Debug包含secret/token，BFF绝不deriveDebug或记录此类型/原错误。应用包装Bearer在Drop擦除token，减少额外bundle克隆；最后拥有的Client与临时授权Client擦除secret/PKCE；SDK内部临时分配生命周期由库控制，未声称所有内存副本均可保证擦除。

保护业务API每次introspection读最新权威状态，无active成功缓存；active=true必须匹配当前sub/client_id及完整exp/iat/scope/token_type。共享PG行锁覆盖刷新HTTP与加密新令牌提交；同BFF session多个实例串行，失败失效，旧refresh不无限重试。BFF数据库长期令牌加密绑定namespace/sessionUUID；A/B Cookie、client、callback、scope同意与数据库namespace独立。本应用退出先提交共享行锁内失效，再以服务端Basic对固定/oauth/revoke一次请求；网络失败仍清Cookie且明确failed，没有后台重试队列。

## 源文件 SHA-256

| 文件（精确crate源） | SHA-256 |
|---|---|
| openid0.25.0 Cargo.toml | bfab77c44fdd9385558a08cd4f495f3336e52557672433ea9f2c0d55c8007502 |
| openid0.25.0 src/client.rs | a26455aa5bebe5d6ee0d9ce0db3e1f30414c2979e5d2ec07df46e8798aafb03b |
| openid0.25.0 src/discovered.rs | b2f27a08cc1e07524cb86472872a8b3b35b3370351a6b3ba9e1714c2996d7fe7 |
| openid0.25.0 src/validation.rs | 2bcb4832247477559a3cfd72733046b996911e66c6e8a2ffd874e175a329dfa7 |
| openid0.25.0 src/pkce.rs | 0af27cfc089df8307a6c78c3b4ebf34aafe09c7036833029395dfa514e8bf968 |
| biscuit0.7.0 Cargo.toml | 04ca59d29959fe5f19f9a0820b35545333bf8e877ba3ae1bd254eb8a0de0fdbd |
| biscuit0.7.0 src/jws/compact.rs | deeb141c116ef037d911f1c305abadb309e80dc58e08cf327450136194d13b0e |

这些记录来自cargo info、crates.io/GitHub官方元数据和本地下载源码读取。维护/许可证核查已执行；完整安装、锁、安全扫描与真实互操作结果将在对应T13证据更新，不提前标通过。

实际安装及锁文件验证：cargo check demo-bff成功，精确锁openid0.25.0/biscuit0.7.0/reqwest0.13.5，cargo audit --no-fetch --json退出0（364依赖、0漏洞、无warnings），[缓存扫描](cargo-audit-cached.json)。这是缓存公告库扫描，未声称在线更新或生产发布完成。
