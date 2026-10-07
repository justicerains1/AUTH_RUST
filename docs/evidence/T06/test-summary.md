# T06 密码登录与会话验收

2026-10-08，Linux/WSL2。实现verified密码登录/有限MFA分支、me、本人设备分页、指定设备/当前/全设备退出，并在同事务撤销派生grant/token。未知账号使用预计算dummy，已知校验编码中的参数，旧密码hash升级不更改credential_version；版本/状态在密码校验后user锁下重读。

真实HTTP/PG/Redis测试通过：12h绝对期限/安全Cookie/CSRF轮换、固定旧预认证拒绝、OAuth数据库事务绑定原参数迁移、有效main-only重新登录、MFA新预认证且没有ordinarysession、跨用户设备拒绝、派生token即时撤销、cursor分页和重复退出；密码POST受控等待用户锁，禁用先提交后无新session。过期测试显式数据库timestamps。

最终Playwright2case全部通过：密码登录访问账号/退出后无法访问、MFA账号密码成功仍不能访问账号。22项前端测试、62项脚本测试、Rust单元与fmt/Clippy/TS/ESLint和build均0。production配置样本__Host/Secure/HttpOnly/Path/Lax/noDomain验证通过，未部署生产。

实现过程中OptionDigest与query类型/Clippy告警真实失败保留；修后复测通过。初始远端CI Windowsunit及T03失败，日志未取得；本机回归通过不代替跨OS全量验收。原文后续T07～T24继续按依赖，生产环境用户明确暂未准备。
