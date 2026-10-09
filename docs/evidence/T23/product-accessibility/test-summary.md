2026-10-09最终全量中的[双浏览器12案例](2026-10-09T03-29-23-181Z.json)已0，四宽布局/文本缩放及完整客户端强认证秘密焦点链保留；此为当前完整回归的本地自动化范围，真实读屏/实体手机/Safari仍待验。

# T23 产品页面可访问性与 Linux Firefox 自动检查

2026-10-09追加定向验收后，最新完整运行2026-10-08T19:04:12Z～19:07:21Z为两浏览器**12/12通过**、退出0，见[最新完整结果](2026-10-08T19-04-12-019Z.json)。原四宽80组/文本缩放20组保持，另新增每浏览器一个真实客户端创建→密码/恢复码强认证→秘密关闭→轮换强认证→显式确认→秘密关闭的完整键盘用例。新版源码SHA见[当前源码清单](source-manifest.json)；以下10例报告保留为历史范围，不替代最新12例。

2026-10-08T14:59:38Z～15:02:46Z 完成：两种真实浏览器、10 个用例全部通过，退出码 0，无 skipped。最终结果为 [运行报告](2026-10-08T14-59-38-709Z.json)，受测源码 SHA-256 见 [源码清单](source-manifest.json)。本结果补充 T16～T19、E22/E23 的本地自动化证据；五类浏览器人工矩阵、屏幕阅读器、实体设备和正式发布关卡仍需按原文档验收。

| 实测浏览器 | 版本 | 四宽账号页 | 四宽后台面板 | 200% 文本缩放 |
| --- | --- | --- | --- | --- |
| Playwright Chromium，Linux 无头 | 153.0.8010.12 | 6 页 × 4 宽 = 24 组 | 4 面板 × 4 宽 = 16 组 | 6 页 + 4 面板 = 10 组 |
| Playwright Firefox，Linux 无头 | 155.0 | 6 页 × 4 宽 = 24 组 | 4 面板 × 4 宽 = 16 组 | 6 页 + 4 面板 = 10 组 |

四种视口宽度为 360、390、768、1440 CSS px。账号页为 `/me`、`/me/sessions`、`/me/grants`、`/me/password/change`、`/me/mfa`、`/me/passkeys`；后台面板为用户、应用客户端、管理员、安全审计。真实查询加载完成后检查，80 组普通字号、20 组放大字号均无页面横向溢出、无可见且无名称的表单控件、无活动动画或 transition；`prefers-reduced-motion: reduce` 实际匹配。逐项数据见 [Chromium 账号](chromium-account-layout.json)、[Firefox 账号](firefox-account-layout.json)、[Chromium 后台](chromium-admin-layout.json)、[Firefox 后台](firefox-admin-layout.json)。

每个浏览器均实际完成五个用例：

1. 真实注册、Mailpit 邮件、显式邮箱确认、密码登录、找回及重置后重新登录。按钮和输入通过 Tab/Shift+Tab、Enter 与键盘输入操作；邮箱确认不会自动发请求；密码重置成功后 `/me` 仍为 401；localStorage/sessionStorage 为空。字段错误有 `aria-invalid`、关联的 `role=alert`；状态区域有 live-region 语义。
2. 六个账号页四宽、200% 文本缩放与 reduced-motion；放大字号下退出弹窗可用键盘取消，并返回开启按钮。
3. 真实错误密码身份确认：初始焦点位于取消按钮，错误响应产生 assertive alert、清空密码；Tab 留在 dialog，Escape 返回开启按钮，已有账号会话保持可用。
4. 真实管理员恢复码登录、四个后台面板四宽与200%文本缩放、纯键盘切换、客户端无效输入的 assertive alert。
5. 管理员高风险确认弹窗：初始取消焦点、Tab 焦点约束、Escape 关闭及回到开启按钮；取消过程没有任何非 GET 请求。

所有 HTTP、密码/MFA、恢复码消费、数据库、Redis 和邮件 worker 均为真实实现，无 API 响应 mock。每次使用 UUID 隔离的测试 schema；5320/5321/5322 分别用于页面/API/私有测试控制；控制端点只按用户、摘要与 `consumed_at IS NULL` 读取未消费测试恢复码，未修改生产认证规则。正常退出清理 schema、私有目录与本次进程，不停止共享 PostgreSQL/Redis。证据仅包含公开方法、布尔结果与脱敏失败信息，不保存密码、OTP、Cookie、token、seed 或私钥。

实际发现并修复了两处产品缺陷：`AdminAction` 没有 Radix `Dialog.Trigger`，取消后焦点未返回开启按钮，现明确保存并恢复开启按钮焦点；账号安全头部的长邮箱 flex 项保留 `min-width:auto`，Chromium 在200%字号下右边界超过390px视口，现仅对此项允许收缩与换行，没有隐藏或裁剪内容。新增后台单元回归验证 Escape/取消后的焦点恢复且不执行 mutation。

历史失败完整保留：首次 schema 名称超过数据库长度限制；首轮页面运行 2/10，包含密码标签定位歧义、实际焦点缺陷与缩放溢出；修复定位与焦点后 8/10；补断行后 9/10。Firefox 剩余失败是在页面最后一个控件继续按 Tab 不循环：三按钮空白页面也可复现无头 Firefox 末端保持焦点，而 Shift+Tab 正常返回。测试据目标 DOM 位置选择 Tab/Shift+Tab，保持真正键盘操作与100步上限，无 programmatic focus；随后 [原范围10/10](2026-10-08T14-53-40-185Z.json)，扩展全部文本缩放与真实数据加载后最终仍10/10。早期报告与诊断文件以时间戳保留。

运行命令与补充静态/单元验证均实际退出0：

```sh
PATH=/root/.cargo/bin:$PATH node tests/accessibility/product.mjs
node node_modules/typescript/bin/tsc --noEmit --project tests/e2e/tsconfig.json
node node_modules/typescript/bin/tsc --noEmit --project apps/identity-web/tsconfig.json
node node_modules/eslint/bin/eslint.js tests/e2e/product-accessibility.spec.ts tests/e2e/product-accessibility.config.ts tests/accessibility/product.mjs apps/identity-web/src/pages/admin/AdminShared.tsx apps/identity-web/src/pages/admin/admin.test.tsx --max-warnings 0
cargo clippy --package identity-server --test t23_accessibility --locked -- -D warnings
cd apps/identity-web
node ../../node_modules/vitest/vitest.mjs run src/pages/admin/admin.test.tsx
```

后台单元测试为5/5（包含新增取消焦点案例）。运行前需本地测试 PostgreSQL/Redis/Mailpit 和私有开发配置；浏览器安装命令为 `node node_modules/playwright/cli.js install chromium firefox`，干净Linux环境可加 `--with-deps`。安装目录中浏览器二进制 SHA-256 随最终运行报告记录，不将 Playwright Chromium/Firefox 等同于用户手测的官方稳定版完整矩阵。

验证边界保持明确：200% 是390px视口将根字号16px设为32px的**文本缩放**；无头浏览器 Control++ 未改变 viewport/DPR，因此本报告不声明完整桌面浏览器200% zoom已通过。axe WCAG2/2.1/2.2 AA 标签无自动 violations，但部分 `color-contrast` 规则返回 incomplete，需人工复核。ARIA检查不证明屏幕阅读器实际播报；真实 NVDA/VoiceOver等播报、人工对比度/完整放大、密码管理器、真实剪贴板与系统返回、Safari/移动Safari/移动Chrome、实体Passkey均不能据本报告填通过。无头Firefox的键盘/页面证据仅覆盖这里实际执行的流程，完整MFA/Passkey/SSO五类浏览器矩阵仍按 acceptance.md 保持待验收/阻塞。

追加用例的真实缺陷与修复：两浏览器都曾在客户端创建成功后的秘密dialog按Escape关闭后失去原按钮焦点，见[首轮2例失败](2026-10-08T18-29-51-838Z-browser-failure.txt)。`ClientsPage`现在保存创建按钮ref，轮换结果通过`AdminAction`的可选opener参数取得最初真实开启按钮，dialog关闭明确返回仍在DOM的按钮；按钮不在时回到创建按钮。只保存元素引用，不改变认证、权限、秘密显示或存储。后台单元保留5/5，补充创建secret Escape返回与opener传递断言。

最新新增case每浏览器实际验证：创建前真实密码+未消费恢复码认证，完成后回创建按钮，**再次显式提交才POST201**；秘密dialog初始关闭焦点/Tab约束/Escape返回创建按钮、秘密从DOM消失；同一新客户端行的轮换Action→Reauthentication密码/恢复码→Action取消焦点，认证不会自动轮换，**显式确认才POST200**；轮换新秘密不同，关闭后回对应轮换按钮，localStorage/sessionStorage为空。详情见[Chromium连续dialog](chromium-client-dialog-sequence.json)、[Firefox连续dialog](firefox-client-dialog-sequence.json)。秘密正文仅在测试内比较，不写报告。

测试隔离失败也保留：最初两browser共用admin导致第6次密码确认正确触发5次/分钟限流；之后同账号布局读取占用30次/分钟admin预算使后续列表429，新增client让旧case全页按钮匹配到两项。见[密码预算失败](2026-10-08T18-37-36-593Z-browser-failure.txt)、[旧case定位与后台预算失败](2026-10-08T18-56-09-625Z-browser-failure.txt)。最终将布局/旧取消、新dialog分别使用固定browser/场景专属真实admin与合格TOTP、membership，每账号通过标准`RecoveryCodes::generate`产生完整10码；私控只接受四固定枚举并按对应user/hash/未消费状态返回。旧取消case限定原transaction行，新case限定自己创建行。没有清Redis预算、改限流、删除原断言或mock API。中途拆分5码夹具不作为最终标准证明；最终12例均采用每账号完整10码。

根命令默认必须12 passed。诊断命令 `node tests/accessibility/product.mjs --focus-client-dialogs`只运行新增两例并记录`focusedClientDialogsOnly:true`，不冒完整验收。最新完整报告字段为false；失败命令输出脱敏保留，原历史未删除。
