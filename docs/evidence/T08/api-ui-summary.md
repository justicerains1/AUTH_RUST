# T08 因素 API 与页面记录

日期：2026-10-08（Asia/Shanghai）。本模块实现TOTP/恢复码HTTP边界、前端登录/近期认证续接与本人因素管理。TOTP密码算法和数据库原子状态由独立核心/仓储模块实现，复用已有CSRF、Origin、Cookie、限流与安全错误。

## 已实现范围

- 七个计划端点：auth MFA TOTP/recovery verify、本人reauth TOTP、TOTP enrollment/confirm/delete、recovery codes regenerate。所有状态变更要求Origin/绑定CSRF，JSON拒未知与重复字段，挑战/IP预算依旧真实Redis原子计数。
- 挑战id/code之外，客户端不能提供user、purpose、amr、strong_at等权威属性。库根据数据库purpose/当前preauth或session、user/version、期限与次数判断；专用reauth端点向仓储传reauth_only，在任何因素更新/消费之前拒login用途，不以提交后的响应检查保护事务。
- MFA登录只有成功因素证明后返回authenticated、生成真实随机session/CSRF与预认证Cookie并迁移授权事务；reauth成功仅更新近期strong状态，保留原auth_time及绝对session到期。密码单独不满足已有MFA的安全操作。
- 首次绑定要求本人近期密码；待绑定种子加密存储，只有该session的totp_enrollment挑战可确认。成功一次显示十个恢复码。已有MFA关闭与重建恢复码要求最近五分钟强认证，ReauthFailure只返回实际已配置方法。
- AuthAppState新增new_with_clock，Security/Session/Password/MFA使用同一注入Clock，production默认SystemClock。数据库到期与TOTP时步边界可真实验证，不靠长sleep或修改replay counters。

登录页面支持实际TOTP/恢复码续接、完整粘贴、一次提交、错误清验证码；过期/次数耗尽引导重新开始。本人 `/me/mfa`提供当前密码与第二因素近期认证、二维码/手动密钥、确认启用、关闭TOTP和恢复码重建；密码修改页面可完成实际第二因素后才解锁。

二维码与恢复码只在当前组件内存，完成/取消/关闭时清理，不存browser storage、不写遥测、不保存秘密截图。恢复码复制由用户点击写剪贴板，提示可信保存及共享设备清理。二维码矩阵以native SVG绘制、依赖懒加载，详见 [依赖核查](qr-dependency.md)。

## 已执行验证

| 检查 | 实际结果 |
|---|---|
| server编译与Clippy all-targets --locked -D warnings | 退出0 |
| cargo fmt工作区检查 | 退出0 |
| 身份前端TypeScript strict/ESLint | 均退出0，0warning |
| 身份前端Vitest | 6文件、29项全通过 |
| 身份前端production build | 退出0，MFA页面与二维码依赖独立lazy chunk |
| npm audit | 退出0，0漏洞 |
| T08真实集成 | 退出0，四必要案例及补充边界通过 |
| T08真实浏览器 | 1项完整因素流程通过，0失败 |

新增3项前端测试覆盖完整验证码粘贴与失败清码、恢复码正确端点/挑战失效重新开始、用户剪贴板复制且不写storage。API spy fixture仅验证组件行为；身份认证与数据库原子消费由真实集成证明。

[集成](integration.txt)使用独立RFC HMAC客户端、真实PG/Redis：错误验证码不启用、正确绑定/十恢复码、MFA登录才产生会话、密码近期认证不能关闭/重建、TOTP同一步十并发仅一成功、新挑战不能复用last_step、五失败耗尽不可复活、恢复码十并发单次与旧集合失效，以及SystemClock真实时步确认。

测试多场景因真实限流预算触发429；只定向清理本轮随机AEAD-HMAC命名空间内单个登录账号/本地因素IP预算用于场景准备，没有FLUSHALL或修改TOTP/恢复码重放状态。原失败证据保留；限流本身由T04真实独立案例验证。

[E2E](e2e.txt)完整真实流程包含TOTP绑定、第二因素登录和恢复码单次使用。截图/trace关闭，种子/验证码/恢复码仅测试进程内存，不输出实际秘密。有限挑战刷新或浏览器返回若内存已丢失，安全重新开始认证；没有持久化挑战或假成功恢复身份。

最终任务结论及代码推送由根整合验收。T09 Passkey能力不在本模块声称完成。
