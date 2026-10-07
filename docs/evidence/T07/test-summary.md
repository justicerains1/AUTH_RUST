# T07 密码重置、修改和安全通知

2026-10-08。真实PG/Redis/API/lettre SMTP/Mailpit测试完成四PWD案例：reset后新密码/旧密码、全部设备和派生授权状态，GET不消费及按钮确认，10同步HTTP单次消费和跨用途拒绝，TOTP/Passkey保持、MFA仍有限挑战、same-session显式近期密码证明与5分钟边界、强认证不足拒绝、审计失败整体rollback。Worker重置/安全通知实际送达并清密文。

Playwright两个完整真实流程通过：请求邮件→fragment清除→明确确认重置→登录新密码；登录→确认当前密码→修改→重新登录。26前端测试、62脚本测试、Clippy/fmt/TS/ESLint、Rust单元及production build通过；开发库0003迁移实际0。

首次测试禁止一切Set-Cookie误把清主cookie当自动登录，改为只禁止有效身份cookie且检查无session；构造过期证明时调整原sessionauth_time以满足生产CHECK。失败脱敏报告保留。HTTP十并发存在Argon2队列503但仅一次成功、action最终一次消费；不以503冒消费。真实刷新端点尚未在本任务实现，其并发在T12/T20补实际接口回归，当前真实仓储派生token立即失效。
