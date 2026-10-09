# T19 用户、客户端、管理员成员与提交中状态实测

最新完整入口 `PATH=/root/.cargo/bin:$PATH node tests/integration/T19.mjs` 于2026-10-09T02:17:49～02:19:08 UTC实际退出0，原3个T19案例与新增4个管理案例全部通过，见[入口结果](../e2e.txt)。新增[源冻结报告](2026-10-09T02-18-13-064Z.json)记录9项源码运行前/后SHA一致及实际Cargo测试程序摘要，4场景0失败；使用真实Chromium/页面/API/PG/Redis，无响应mock、无trace/Cookie/OTP/秘密输出，不修改生产源码。

| 新增场景 | 实际操作与后态 |
| --- | --- |
| 用户禁用/启用/全会话撤销 | 普通目标实际密码登录me200；管理员UI经实际密码+未消费恢复码强认证及显式确认PATCH禁用200，旧设备me401；UI启用200后目标新登录me200；UI全会话撤销POST200，目标me401，用户API仍active |
| 客户端精确编辑/停启 | UI编辑名称、两精确登录回调及退出回调，真实强认证后再次显式保存PATCH200；停用/启用各经明确确认，UI/实际API enabled及URI字段准确持久 |
| 管理员授予/移除/最后保护 | 候选verified+TOTP实际密码/恢复码登录，授予前adminAPI403；UI授予后200、移除真实DELETE204后403；夹具准备最后一个可用管理员后UI移除返回409/ADMIN_LAST_MEMBER，错误提示明确，取消后仍可adminAPI200 |
| pending不能重复或取消 | 私控持真实oauth_client行锁，生产PATCH实际`pg_blocking_pids`等待；提交和取消按钮disabled、重复鼠标双击及Enter无第二PATCH、Escape不能关闭pending；提交前enabledtrue/audit0，放锁后PATCH200、enabledfalse/audit1且请求仅1次 |

每场景使用固定独立管理员、真实AEAD加密TOTP与标准`RecoveryCodes::generate`10码集合，私控只返回固定场景DB确认未消费的恢复码/公开fixture ID。浏览器必须真实密码登录和恢复码强认证，没有SQLstrong标记或改客户端`identityConfirmed`。普通用户目标、成员候选和客户端均在UUID独立测试schema中，未触生产或共享数据。UI操作不是只看截图：同时检查真实状态码、下次权限及数据库审计/等待。

新增harness端口5341API/5342私控，UI5340；它只允许固定users/clients/members/pending枚举，pending只锁独立固定client，last-admin准备只在UI已移除固定候选后关闭其他测试管理员；最后恢复测试admins。最后管理员场景是前置状态准备，真正拒绝由实际生产DELETE事务返回，不用私控模拟响应。停止时任务退出并释放锁、删除schema和私有目录，没有停止PG/Redis/SMTP共享依赖。不会宣称生产管理员操作或设备验证。

失败历史：[首次报告](2026-10-09T01-51-41-371Z-failure.json)及[脱敏诊断](2026-10-09T01-51-41-371Z-browser-failure.txt)3/4，成员移除后权限检查过早读到200。测试当时只等row不存在，query刷新loading会暂时让row消失，未等待DELETE完成；改为实际DELETE204→dialog关闭→API403，随后[4场初次通过](2026-10-09T01-57-43-455Z.json)。后续补pending重复pointer/audit与409错误码断言，真实[强化4场通过](2026-10-09T02-02-27-463Z.json)，再入口顺序3+4及源码前后验证通过。没有修改生产授权/事务/限流规则，没有削弱403或last-admin断言。

实现与命令：

```sh
PATH=/root/.cargo/bin:$PATH node tests/integration/admin-management.mjs
PATH=/root/.cargo/bin:$PATH node tests/integration/T19.mjs
node node_modules/typescript/bin/tsc --noEmit --project tests/e2e/tsconfig.json
node node_modules/eslint/bin/eslint.js tests/e2e/admin-management.spec.ts tests/e2e/admin-management.config.ts tests/integration/admin-management.mjs tests/integration/T19.mjs --max-warnings 0
cargo clippy --package identity-server --test t19_admin_management --locked -- -D warnings
```

新增入口[runner](../../../../tests/integration/admin-management.mjs)、[浏览器用例](../../../../tests/e2e/admin-management.spec.ts)、[专属harness](../../../../crates/identity-server/tests/t19_admin_management.rs)与原T19顺序整合，缺脚本/工具或非4passed不可绿。以上静态检查和实际入口均0，diff-check0。原十万用户分页、秘密创建/轮换一次显示、普通用户403及两浏览器焦点范围证据仍保留；本次新增主要覆盖T19.01/.02/.04/.06的实际危险操作与pending，不冒全部五浏览器/读屏/真实生产通过。
