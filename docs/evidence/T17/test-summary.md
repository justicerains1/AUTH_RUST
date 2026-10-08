# T17 认证产品流程验收

2026-10-08，父17da861，Linux/Rust1.98/Node22、PG17/Redis7.4/Mailpit与Chromium153。三项页面案例与6项真实E2E通过；前置T09整体因实体设备待验收，按原依赖T17任务保持待验收；代码版本由本次Git提交记录。

采用已确认T15视觉：首页、注册/登录、邮件确认和密码恢复完整页面，统一系统品牌与单主入口，四宽360/390/768/1440的20个已挂载页面无水平溢出/axe A/AA规则违规，键盘跳转和reduced-motion验证。4个首页安全截图只空闲品牌页，根实际查看390手机稿；原视觉规范顶部过时未确认文字同步已确认记录，不重新要求设计批准。

真实注册→Mailpit验证fragment清除→明确确认→密码登录；MFA真实TOTP、恢复码切换/贴码清秘密、过期重启；浏览器真实WebAuthnAbort取消不提交assert并可密码回退。真实Redis429根据Retry-After倒数禁重发，零后不自动POST。服务端验证OAuth事务ID跨公开流程保留，过期/错误GET不能靠缓存跳授权。

邮件秘密仅短期ref（验证30min、重置15min），成功/失效/离开清理，不放持久storage；错误保留邮箱等非秘密。新增密码重置完成状态，需要明确“前往登录”，旧T07真实浏览器用新动作回归；旧T05/T06/T07/T09各2案例实际通过，虚拟Passkey仍真实签名和负向验证。

最终check/unit/build、docs/OpenAPI、65工具测试退出0；身份前端50项、A/B各6项，MFA/Passkey组件9项包含其中。6E2E和20axe通过，详细e2e.txt/test-boundaries.md/final-check.txt/unit.txt/build.txt及screenshots/。

失败记录保留：测试误填lazy导航旧Login表单已独立浏览器复现并等待新唯一h1；固定MFA时钟不能领取实时outbox，Worker使用真实SystemClock；测试expire SQL漏bind已改严格rowsaffected/错误，role定位与加载截图改等待实际页面。没有放宽RHF、认证、SMTP或过期策略。

T09实体Passkey/Safari及屏幕阅读器手工验收仍待执行；当前案例证明Linux浏览器真实流程与移动宽度，不假称生产或实体设备验证。发布关卡T20～T23未放行。

任务状态核对：文档检查禁止前置未通过时把T17标通过。没有放宽依赖，当前提交的是已测试代码和真实案例证据，任务结论仍待验收；T18可以进行独立实现，但整体验收受同前置约束。
