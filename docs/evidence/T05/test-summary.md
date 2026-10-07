# T05 注册、验证与邮件真实验收

最后验收：2026-10-07，Linux/WSL2；本次初始化Git提交由Git记录。T05已完成，按用户要求停止T06及后续实现，仅整理仓库、进度和推送。

## 真实结果

- 新邮箱注册提交user/verify action/encrypted outbox/audit同事务；POST统一202，重复注册不改现密码/已有动作。
- Worker真实lettre SMTP投递Mailpit，提交领取租约后才发送；成功清encrypted_params，验证链接只固定ISSUER fragment。
- GET链接不消费；用户按钮POST才verified并单次consume；重复/重发旧动作409，新动作一次成功。验证不创建普通session，不启用disabled账号。
- 已有/未知邮箱注册与重发同202正文，邮件目标预算超额仍202；弱密码/未知字段/过期/交叉用途/禁用状态/强制审计失败rollback等真实负向通过。
- 真实停止Mailpit，outbox保留并安排重试；恢复并显式数据库时间构造due后送达，无长sleep等待到期。
- 最终Playwright两case全部执行：注册→真实邮件→fragment清除→GET不提交→点击验证，以及重发旧链接失败/新链接成功。2 passed、0 failed，证据e2e.txt。不保存私密trace/视频/邮件截图。

## 修复与证据

原生fetch作为类方法调用导致Illegal invocation；绑定globalThis并增加receiver测试，真实浏览器复测成功。第二case原误查role=status，而正确409页面为alert；修正测试后两个流程真实通过。保留before-alert-fix与初次网络失败脱敏报告，不把失败改写成成功。

check、unit、Rust/frontend production build、真实integration、最终e2e退出0；18前端测试与工具59项通过。依赖lettre0.11.23许可和来源见worker-dependencies.md。在线RustSec更新network失败，使用本地已更新公告缓存对新锁实际278依赖、0漏洞、warnings为空，明确缓存commit和时间见rustsec-cache.md。

当前只完成注册与邮箱验证，密码登录/MFA/Passkey/OIDC/BFF/后台、全面性能安全、生产SMTP/备份/发布仍未实现或未验收；不能以开发送达证明生产可发布。
