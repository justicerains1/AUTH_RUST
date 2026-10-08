# T07 邮件 E2E 隔离修复

2026-10-08，父e6bd9c0。Windows环境修复后的集成CI推进到T07E2E；本地真实复现：重置后仍留原页面，API正确拒绝无效动作。测试使用固定browser-reset@example.test，Mailpit残留上轮同邮箱邮件，新outbox尚未送达时resetToken取出旧token，和当前隔离schema不匹配。失败证据before.txt保留。

测试请求前记录Mailpit消息ID，仅接受本次请求后新消息；不删全局邮件、不跳过后端、不改变密码/动作校验。真实PG/Redis/Mailpit/Playwright复测2passed/0failed，见after.txt及T07/e2e.txt；修改只涉及测试邮件选择。
