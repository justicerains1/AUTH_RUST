# T13 真实双 BFF 测试边界

真实身份 API、PostgreSQL identity_test 临时 schema、Redis，以及同一 Rust demo-bff 包的 A/B 和第二 A 实例；三个 Vite 页面使用各自真实代理，维护中的 Rust OIDC 客户端实际执行 discovery、state、nonce、PKCE、授权码交换和 ID Token 验证。测试结果为 4 passed / 0 failed。

A 登录后 B 复用身份会话但首次独立同意；身份平台全部退出后 A/B 下一次保护请求拒绝。浏览器不发 token 交换请求，不在存储或 /bff/session JSON 获得 access/refresh/ID Token/client secret；仅检查 Cookie 元数据，不保存秘密值。

跨 A/A2 的十个真实 HTTP 请求共用数据库会话行锁，只有一次 refresh 轮换。测试准备通过私有控制端点把加密 token 的本地到期设为合法未来时间以触发刷新，没有改变身份令牌的权威状态或重放计数器。

真实停止 Redis：introspection 故障返回503，恢复后正常；本应用退出先提交本地失效，再一次请求撤销授权，A 的授权实际失效，B 保留。撤销网络失败返回 revocation_status=failed 并清 Cookie，不声称后台重试队列。需要刷新时的真实网络故障也返回503并清本地会话 Cookie。

篡改真实 OAuth 回调 state 被拒绝且不创建 BFF 会话；平台退出使用固定身份地址，无 ID Token hint 暴露给浏览器，并等待用户明确确认后退出。浏览器 trace、视频与截图关闭；诊断中的长令牌和回调 query 脱敏。实体生产部署、后续高可用容量及 T20 的全面互操作扫描不由这些案例替代。
