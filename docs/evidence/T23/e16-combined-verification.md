# E16 管理员禁用与 OAuth 凭证组合验收

2026-10-08 UTC，Linux/WSL2，真实PG/Redis/Axum HTTP与实际管理员CLI。源码为[tests/t14_admin.rs](../../../crates/identity-server/tests/t14_admin.rs) `admin_disable_revokes_oauth`，由`cases`在真实管理员密码+TOTP近期强认证后调用。编辑起点为`0f190e6`；本次实际执行包含随后未提交的新增测试，最终提交与包含新增测试的完整回归由根记录，此处仅报告局部真实执行。

同一目标账号先实际密码登录（/me200），管理员API创建受控client，真实authorize→同意→Basic/PKCE code exchange200，再实际refresh200取得当前凭证，access与当前refresh introspection均200/active=true。以轮换后的尚未消费refresh做后续检查，避免旧refresh重放造成假阳性。

真实强认证管理员PATCH该用户status=disabled成功200后：旧/me401，access与当前refresh introspection精确为`{"active":false}`；当前refresh请求400/`invalid_grant`，无Set-Cookie和access/refresh/id_token字段，目标用户oauth_tokens总数与禁用前相同。禁用使用实际管理员API，没有直接SQL改status；SQL只准备初始账号和核对记录数。秘密、凭证和用户名不进入新增PASS输出。

实际命令`PATH=/root/.cargo/bin:$PATH npm run test:integration -- --task=T14`：

- 首次13:30:27.885～13:30:47.193 UTC退出1，Rust测试101：协议GET使用默认reqwest Client，跟随authorize302到无SPA的consent路径，最终404而断言期望302。这是测试客户端失误，生产路由存在。仅新增独立`redirect::Policy::none()`协议client，保留原Browser与生产逻辑；[失败摘要](../T14/integration-failure.txt)、[脱敏诊断](../T14/integration-diagnostics-1791466247191.txt)保留。
- 修后13:35:27.316～13:35:49.511 UTC退出0，顶层真实集成测试1通过/0失败/0ignored；[成功记录](../T14/integration.txt)含完整E16关键PASS和原T14管理员/并发案例，未以局部结果覆写失败文件。
- `cargo clippy -p identity-server --test t14_admin --locked -- -D warnings`、定向rustfmt及`git diff --check`均0。最初shell缺rustfmt路径返回127，随后显式工具链路径成功，未修改生产代码。

测试API监听随机loopback端口，数据库仅本次identity_test随机schema，结束后清自身schema；未停止共享PG/Redis/SMTP。runner只收集明确PASS行到公共证据，错误原始输出先替换数据库/密码/长opaque字段再保存；运行时私有keys/SMTP文件位于忽略目录、0600并由runner清理。没有提交完整token/Cookie、私钥或密码，也没有生产部署。

E16的本地同账号联合断言已补齐；正式域名下管理员禁用冒烟及包含此新增测试的最终全面回归仍须单独留证。
