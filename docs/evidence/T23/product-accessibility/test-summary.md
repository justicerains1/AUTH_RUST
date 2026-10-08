# T23 产品页面可访问性与 Linux Firefox 自动检查

2026-10-08T14:59:38Z～15:02:46Z 完成：两种真实浏览器、10 个用例全部通过，退出码 0，无 skipped。最终结果为 [运行报告](2026-10-08T14-59-38-709Z.json)，受测源码 SHA-256 见 [源码清单](source-manifest.json)。本结果补充 T16～T19、E21/E22 的本地自动化证据；五类浏览器人工矩阵、屏幕阅读器、实体设备和正式发布关卡仍需按原文档验收。

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
