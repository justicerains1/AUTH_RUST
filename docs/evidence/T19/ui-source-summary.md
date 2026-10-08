# T19 管理后台 UI 验证摘要

日期：2026-10-08。源位于apps/identity-web/src/pages/admin、lib/admin-api.ts及共享ReauthenticationDialog；AdminPage入口加载四个后台子页面。真正权限以每个后端handler及数据库事务为准；普通用户打开页面不会得到后台数据，403单独呈现，近期认证过期可进入真实强认证流程。

用户列表按完整邮箱与状态精确查询，客户端/管理员/审计均按20条cursor分页；审计固定from/to窗口最长31天，不取全部历史。API响应由明确Zod schema解析，输入名称只作React文本，不注入HTML；异常响应不显示占位成功。

高风险操作单流程：先说明影响→身份确认→独立明确重确认。只有最终用户点击才提交操作；身份成功不自动重放原POST。共享弹窗初始焦点取消，关闭返回原触发按钮，取消清密码并中止请求；Factor/Passkey互斥、防重复与Retry-After保持。成功结果由实际服务器响应决定。

客户端秘密通过直接api请求得到，仅存局部state；轮换时先临时ref，确认弹窗关闭后才显示结果，避免双modal。关闭立即清state/ref，不进QueryClient或MutationCache，不localStorage/sessionStorage保存，不向后端重新获取秘密。失败保留原客户端状态与非秘密输入。

实际模块测试：后台3项行为测试（恶意文本转义、一次秘密弹窗与查询缓存无secret、强认证后独立重确认），共享焦点1项测试（open初始取消→cancel回原按钮且无POST）通过。最新TypeScript/限定ESLint退出0。根完整单测产物 [unit.txt](unit.txt) 记录identity-web58、demo-a6、demo-b6通过；[final-check.txt](final-check.txt)与build产物成功。

真实浏览器验收 [e2e.txt](e2e.txt) 记录3个Playwright场景通过：后台客户端创建/轮换/关闭秘密、普通用户无后台数据、十万真实用户数据分页/恶意名称与审计窗口。原失败、索引计划及ANALYZE前后证据仍保留，不把初始失败复制为通过。来源字段只记录检查方法/结果，不保存真实secret、Cookie或邮件token。

页面测试和axe不代替生产设备、全安全或恢复验收；最终T19结论由acceptance的实际记录填写。T17/T18受真实Passkey设备外部条件约束的状态不因后台UI通过而自动放行。
