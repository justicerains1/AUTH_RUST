# T15 Awwwards 官方案例研究

访问日期：2026-10-07 UTC（上海 UTC+8）。来源由实际 HTTP 与 Chromium 145.0.7632.6 访问核查，原始状态、时间、标题、DOM 摘要、字体样本及截图路径在 [browser-capture.json](../evidence/T15/browser-capture.json)。首先访问 [Awwwards 官方 Site of the Day 索引](https://www.awwwards.com/websites/sites_of_the_day/)，再从其链接访问案例详情与官网。四个案例官方详情及官网均返回 HTTP 200；HTTP 200 不等于全部交互可用。

## 已确认获奖记录

| 网站 | 奖项与日期 | 官方详情 | 官网 | 官方桌面/手机截图 |
|---|---|---|---|---|
| twks | Site of the Day，2026-10-07 | [官方详情](https://www.awwwards.com/sites/twks-1) | [twks.ch/en](https://twks.ch/en) | [桌面](reference-screenshots/twks-official-desktop.png) / [手机](reference-screenshots/twks-official-mobile.png) |
| Lidar Drone Scanning | Site of the Day，2026-10-06 | [官方详情](https://www.awwwards.com/sites/lidar-drone-scanning) | [drone.riotters.com](https://drone.riotters.com/) | [桌面](reference-screenshots/lidar-official-desktop.png) / [手机](reference-screenshots/lidar-official-mobile.png) |
| Santioni Spirits | Site of the Day，2026-10-05 | [官方详情](https://www.awwwards.com/sites/santioni-spirits) | [santionispirits.com](https://santionispirits.com/) | [桌面](reference-screenshots/santioni-official-desktop.png) / [手机](reference-screenshots/santioni-official-mobile.png) |
| EDOLUS | Site of the Day，2026-10-04 | [官方详情](https://www.awwwards.com/sites/edolus) | [edolus.com](https://edolus.com/) | [桌面](reference-screenshots/edolus-official-desktop.png) / [手机](reference-screenshots/edolus-official-mobile.png) |

首批访问开始于 2026-10-07T10:24:36.212Z；EDOLUS 补充访问时间见 JSON 中 visitedAt。官方详情明确显示 “Site of the Day - Oct 7/6/5/4, 2026”。不把同页“Honors nominated”提名当成已获该 Honors 奖项。

## 观察及可迁移原则

### twks

[官网桌面](reference-screenshots/twks-website-desktop.png) / [官网手机](reference-screenshots/twks-website-mobile.png)。实际 DOM 字体 GT Standard，主信息 52.8 px，正文 20 px；黑白正文与项目影像构成节奏，导航有明确项目、机构、服务入口。首屏包含项目视频，当前截帧具有亮绿色和大字叠加。DOM 检测到 8 个 video、1 个 canvas；此次导航至 DOMContentLoaded 加固定等待约 10.481 秒，14 个完成请求，不能据此判断正式加载预算或生产性能。

可迁移：大标题负责表达，小段正文负责解释；主操作保持少量清楚选择。对身份产品，采用系统中文字体与静态关系示意，保持登录入口始终清楚，不引入视频背景与品牌资产。

### Lidar Drone Scanning

[官网桌面](reference-screenshots/lidar-website-desktop.png) / [官网手机](reference-screenshots/lidar-website-mobile.png)。实际字体 Switzer，描述 24 px，技术数据约 47.952 px，章节标题在桌面达 240 px。浅色背景和灰蓝技术文字，顶栏同时放品牌、说明、时间；Gear、Solutions、Capabilities 等内容按章节展开。DOM 有 5 个 canvas、3 个 video；固定等待后的最初截图仍主要显示轻背景与顶栏。约 5.312 秒的采集等待、32 个完成请求仅为研究观察，不代表 LCP。

可迁移：用数字化章节与清楚标题分组复杂信息；给辅助信息稳定位置。账号安全中心采用保护方式、会话、应用三组任务，正文保持可读；重要信息不依赖大型画布或滚动动画。

### EDOLUS

[官网桌面](reference-screenshots/edolus-website-desktop.png) / [官网手机](reference-screenshots/edolus-website-mobile.png)。黑色画布、居中白色品牌标题、进度百分比和 “INITIATE SYSTEM”进入操作，形成明确的场景入口。首批截图停在加载进度，DOM 没有 h1/h2/p 字体样本，因此不推断实际内部排版网格。可观察到 1 个 canvas；采集等待约 13.149 秒、11 个完成请求。补充等待与进入尝试在 [extended-capture.json](../evidence/T15/extended-capture.json) 记录：进入后页面变空白，实际 console 报 `pc is not defined` 两次，因此内部体验受阻，不声称完整流程成功。

可迁移：一个明确主操作能降低初始选择负担；加载必须有文字反馈。身份登录直接呈现表单与备用通行密钥，不要求等待装饰体验或佩戴耳机。账号操作不得以加载场景的完成作为安全判据。

### Santioni Spirits：浏览器适配受阻

[官网桌面阻塞截图](reference-screenshots/santioni-website-desktop.png) / [官网手机阻塞截图](reference-screenshots/santioni-website-mobile.png)。官网标题为 “Not Supported”，实际正文 “YOUR BROWSER IS NOT SUPPORTED”，所以内部 typography/grid/spacing/motion 观察受阻；不依据记忆补写。错误页字体 Charles Rosie Regular 32 px；官方详情的标签包含 WebGL、Sound-Audio、Storytelling，仅能确认官方分类，不能据此声称实际交互已经体验。

可迁移：关键账号流程必须可在支持浏览器中直接使用；不能让复杂图形能力决定能否登录。此案例的真实阻塞也解释了为何本设计使用轻量 HTML/CSS。

## 对照结论

| 维度 | 实际发现 | 用于身份产品 |
|---|---|---|
| typography | twks 52.8 px 主信息；Lidar 240 px章节；EDOLUS居中白字加载 | 品牌首页可有强标题；表单与安全页保持16 px正文，不挤压任务 |
| grid | twks横向导航，Lidar固定顶栏；EDOLUS中心入口 | 桌面12列比例、1280 px最大容器，手机单列 |
| spacing | 大留白建立章节节奏；加载场景占满视口 | 用24/32/48/64间距分组，不用装饰阻塞访问 |
| color | 黑白、影像强调、技术灰蓝 | 文档暖白/近黑/冷蓝，实际测所有文字与输入边界 |
| motion | 视频、canvas存在；Santioni浏览器受阻 | 登录避免视频/WebGL；160 ms反馈，减少动态模式保留文字 |
| mobile | 保存390×844截图，加载与适配局限也保存 | 手机20 px边距，44 px目标，错误文字和恢复入口清楚 |
| performance | 固定等待只是本次采集；无真实LCP/INP报告 | 后续按plan第8.4真实测量，不继承获奖站点成本 |

## 来源许可与边界

访问 [Awwwards Legal Terms](https://www.awwwards.com/terms/)（实际 200；最初错误路径 /about-terms/ 为 404 已记录）。没有从该访问取得可再分发品牌、图片、字体或整页设计的授权。截图仅保存作项目内部来源核查与研究证据，版权归各网站和创作者；公开发布研究截图前另核查授权。新稿不使用这些站点的 logo、图片、视频、文案、远程字体或整页布局。采用的系统字体、中文品牌文案及关系示意由本任务独立制作。

T15.01/02 的官方奖项和证据已完成；T15.03 对可访问部分进行比较，Santioni 内部交互及 EDOLUS 进入后的体验受阻。T15.04/05/06 产物见 [visual-spec.md](visual-spec.md)。T15.07 等待用户评审，T15.08 未冻结。
