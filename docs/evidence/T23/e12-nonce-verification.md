# E12 错误 nonce 的成熟客户端真实验收

2026-10-08T13:33:15～13:33:27Z，`node tests/integration/T11.mjs` 实际退出 0。只补充 `tests/interop/T11.mjs`，没有改变生产协议、ID Token 内容、依赖清单或测试 harness。

捕获同一次真实授权码兑换的成功 HTTP 响应并仅在内存克隆。先以 JOSE 和身份端真实 JWKS 校验 RS256、issuer、audience、过期与原 nonce；再以已锁定的成熟 `oauth4webapi` 3.8.8 处理响应，传入错误 expectedNonce，明确检查失败代码为 JWT claim comparison 且被拒声明正是 nonce。对同份未修改响应传入正确 nonce 成功，并由成熟库完成应用级签名验证，返回的 ID Token 与原 token 相同。

没有再次消费授权码、重新签名或改动其他声明来制造负向成功。公开 [T11 结果](../T11/integration.txt) 保存准确 PASS，响应/token/秘密没有写入证据；ESLint、语法与 diff 检查退出 0。该库是现有 openid-client 的固定传递依赖，未添加新的供应链依赖。最终全量回归还须包含新增断言。
