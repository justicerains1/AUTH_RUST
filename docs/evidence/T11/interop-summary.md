# T11 成熟客户端互操作结果

真实 Node 22 测试使用 openid-client 6.8.8 与 jose 6.2.12，均为 MIT。版本、维护时间与许可证由 npm registry 实际核查；依赖安装记录见 interop-dependencies-install.txt。官方代码来源：

- https://github.com/panva/openid-client
- https://github.com/panva/jose

openid-client 实际读取本系统 discovery，经真实注册客户端的 client_secret_basic、PKCE、state、nonce 执行授权码兑换，验证 ID Token。独立 jose 再使用公开 JWKS 检查 RS256、issuer、audience、时间和正常声明；none/HS256、错误 issuer/audience/key 及过期时间的 token 均被拒绝。测试未打印 JWT、client secret 或 Cookie。

Rust 测试真实 PostgreSQL/Redis/HTTP，检查非法 verifier/client/redirect、十个并发授权码兑换仅一次成功；userinfo 的稳定 sub、scope 字段限制、拒绝 refresh token；JWKS 无私钥字段，discovery 只声明当前已实现授权码能力。真实数据库池关闭后 token 返回503；浏览器Cookie不能替代Basic，不开放CORS。签名失败时授权码未消费，整个事务回滚。

每次使用独立 identity_test 临时 schema，测试私有文件写入忽略目录 .local，退出后清理；interop 网络地址仅测试传输映射，签名 issuer 与回调原样验证。实际通过记录见 integration.txt。

refresh_token、introspection、revoke 与 RP logout 的行为留待 T12。T11 没有将它们的未来能力宣称通过。
