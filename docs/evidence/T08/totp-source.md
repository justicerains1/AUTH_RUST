# T08 TOTP 依赖及算法验证

2026-10-08实际访问crates.io `/api/v1/crates/totp-rs`：当前稳定6.0.0、notyanked、MIT、MSRV1.88。下载源码Cargo.toml许可/仓库与metadata一致，manifest SHA256 `e4e5da007bc0c6b2fea0998bebc5eb0f6e86577378e2b49818c2e9c983bee1bb`。GitHub https://github.com/constantoine/totp-rs 实际未archived/disabled，pushed_at=2026-08-06T13:50:36Z。

精确新增totp-rs=6.0.0，otpauth/zeroize，无二维码图片依赖；种子随机使用既有getrandom，库生成算法避免自行实现HMAC动态截断。源码check明确返回matched counter并说明库不负责单次消费，因此store原子last_step为权威重放防御。

RFC6238官方正文实际读取HTTP200： https://www.rfc-editor.org/rfc/rfc6238.txt ，UTC 2026-10-07T17:59:46.982712，32174字节、SHA256 `82947ed9064450850547f55959dc79d2de775f0fa33f7b3f9622fb6c93e69a7a`。阅读§5.2默认30秒及同step已成功OTP禁止再次接受、Appendix B六个SHA1向量；六位实现按官方8位结果mod百万核对。

`cargo test -p identity-core --lib mfa::tests --locked`真实退出0，2项通过：RFC6238 SHA1六个时间点向量（8位官方向量取6位mod百万）、±1时间步容差/超步/负时间/非法码、160位随机种子/base32往返、10恢复码128位随机摘要唯一及Debug脱敏。`cargo clippy -p identity-core -p identity-store -p identity-worker --lib --bins --locked -- -D warnings`真实退出0。

这些算法测试不代替T08真实数据库原子消费、失败预算、跨用户/用途、登录状态及浏览器流程；最终证据由集成执行器与acceptance填写。没有把测试种子、验证码或恢复码写入本证据。
