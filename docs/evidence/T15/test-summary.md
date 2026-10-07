# T15.01～T15.06 产物与验证记录

日期：2026-10-07；任务执行范围为独立研究、设计变量和具体视觉稿，未进入 T16。目录无密码、令牌、Cookie 或私钥。根 package/Cargo/应用代码未为此任务变更。

## 真实结果

- T15.01/02：实际访问 Awwwards SOTD 索引、四个官方案例和官网；准确核实 2026 年 10 月 4～7 日 Site of the Day。官方及官网 HTTP 全为 200，保存桌面/手机截图。来源及各 visitedAt 见 [reference-cases.json](reference-cases.json) 与 [browser-capture.json](browser-capture.json)。
- T15.03：比较实际可访问 typography、网格、留白、色彩、动效、手机及加载成本；不将固定等待时间冒充 LCP/INP。Santioni显示不支持浏览器；EDOLUS进入后空白并报 `pc is not defined`，已保存证据。研究仅在可观察边界内，见 [reference-study.md](../../design/reference-study.md)。
- T15.04：按 WCAG 相对亮度公式实测对比度。默认正文/辅助/蓝达标；装饰框线仅1.364:1，因此控件加深边界至 #70767D（对白4.589:1）。原默认装饰框线保留；新增语义颜色和理由记录。见 [contrast.json](contrast.json)。
- T15.05：三主页面桌面/手机设计及 HTML 可评审；无远程资产及真实API，示例数据清楚。见 [visual-spec.md](../../design/visual-spec.md)。
- T15.06：按钮、焦点、错误、输入、loading、success、limited、empty、unavailable、确认影响及 reduced-motion 状态稿已交付；字段label/autocomplete和错误关联可检查。

## 浏览器与检查

Node 22.22.1；临时目录 Playwright 1.58.2；Chromium 145.0.7632.6；Linux Ubuntu 26.04 使用下载平台覆盖 ubuntu24.04-x64。最初下载失败因平台未支持；安装系统 libnspr4/libnss3/libasound2 和 Noto CJK 后启动成功。临时浏览器工具未加入根npm依赖。

四设计页在360/390/768/1440共16次检查均无水平溢出。截图已人工检查首页desktop、登录mobile、安全mobile及部分参考图；安全页手机按钮断行已修复并重截图。这里只检查设计产物，不声明实际认证行为、完整可访问性或性能验收通过。

## 未完成验收

T15-DES-01：奖项来源已核实，官网受阻部分据实记录。T15-DES-02：待用户对具体三页面及双尺寸视觉稿评审。T15-DES-03：颜色组合实测与状态覆盖完成；正式人工评审及实际应用WCAG验收未执行。

用户已于2026-10-07回复“采用此方案，进入 T16”，T15.07评审通过、T15.08规范冻结。补充设计标签/键盘/44px目标/reduced-motion实测通过。见user-review.md和keyboard-review.json；实际应用验收仍在T16～T19。
