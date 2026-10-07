# T08 TOTP、恢复码与强认证

2026-10-08。totp-rs6.0.0固定SHA1/6位/30秒±1，RFC6238独立向量测试、20字节随机种子/10个128位恢复码、secretDebug脱敏通过。种子只加密challenge与factor；恢复码只摘要、仅显示一次，UIQRCode原生SVG惰性加载无秘密截图或storage。

真实PG/Redis/API测试通过全部四案例：错误绑定码不启用、正确确认生成十码，密码登录有限挑战；独立NodeCrypto RFC验证码验证后发普通会话；同时间步10并发最多一成功/新challenge不能重放；5错挑战持久consumed不可恢复；同恢复码10并发仅一次成功；重建后旧摘要全部不存在；过期strongwindow后仅密码不能关闭/重建，真实TOTP强认证后合法。专用reauth端点在任何副作用前检查purpose。

完整浏览器长流程1passed/0failed（绑定→密码MFAlimited→TOTP登录→恢复码单次登录）。29前端测试、Rust单元、Clippy/TS/ESLint/fmt及build通过，新增依赖许可核查/audit0，RustSec缓存281依赖0漏洞，不能声称在线公告已更新。

最初测试把刚TOTP登录已有strongwindow误认pwdonly，推进受控clock过5min后验证正确；测试多次passwordlogin/reauth共享真实Redis5/min预算导致429，后按本轮随机派生HMAC键定向准备各场景账号/IP预算，不FLUSHALL、不改生产限流、不重置TOTP last_step或恢复消费。T04单独限流并发测试保留，T08结果只证明因素原子与回放规则。真实SystemClock独立TOTP另case通过。所有失败脱敏报告保留。
