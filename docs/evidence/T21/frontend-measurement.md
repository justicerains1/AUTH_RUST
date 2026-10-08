# T21.07 前端性能独立实测

此产物完成前端测量子模块，不代表 T21 整体通过。T21 前置 T20、后端十万账号/授权种子、各场景十五分钟负载、SQL/哈希队列与资源指标尚需独立验收；不修改 acceptance.md 状态。

## 可重复命令

从仓库根目录执行，先构建当前 production 前端并确保真实本地身份 API 可访问：

```sh
npm run build --workspace @identity/identity-web
node tests/performance/frontend.mjs
```

API 默认 `http://127.0.0.1:8080`，可通过 `T21_FRONTEND_API` 指定其他明确 loopback HTTP origin；不能包含凭据、路径、query 或 fragment。脚本仅将 GET `/api/v1/me` 转发到该真实 API，并要求登录页得到真实匿名 401。禁止用不可用 503 或模拟认证成功代替。脚本不发送密码登录或状态变更，不创建账号、授权或令牌。

生产 dist 由临时 loopback HTTP 服务提供 gzip 内容，不使用 Vite 开发构建。原始 JSON 记录每个产物及源文件 SHA-256、实际请求的 JS 路径和 gzip 大小、浏览器版本/二进制 SHA-256、git 版本及工作区变更标记。每次运行生成带时间的新报告，失败报告保留，不覆盖或改阈值。

## 固定条件和指标方法

- Chrome 固定为 `HeadlessChrome/153.0.8010.12`，脚本核对版本。Playwright 1.63.0 的本地 Chromium，Linux headless；生产用户浏览器指标仍需真实样本。
- 390×844，CDP CPU slowdown 4×；下行 1.6 Mbps（200,000 bytes/s）、上行 750 Kbps（93,750 bytes/s）、延迟 150 ms。
- 首页、匿名登录页各五个新 incognito context；清浏览器 HTTP cache、禁 cache、禁 Service Worker。等待真实页面 h1 和所需网络完成再读指标，页面错误或必需资源失败会拒绝报告成功。
- LCP 使用 PerformanceObserver 最后一个真实 largest-contentful-paint entry；CLS 使用无 recent input 的 layout shifts，按一秒间隔/五秒最大 session window 求最大累计值。每页报告五个原始样本和中位数。
- 首屏 JS 为此页实际加载的唯一 `.js` 资产 gzip 字节之和，包含动态页面和模块预载；不用整个 dist 总体积冒充首屏。
- 本地交互以 CPU 1× 单独测量，实验室交互以 CPU 4×和上述网络单独测量。每次清除旧页面验证状态，测量密码显示切换、空登录输入校验，click 事件时间戳到第二次 animation frame，且验证目标反馈实际存在。各五次原始样本；本地取最大值与 100 ms 比较，实验室取 nearest-rank p75 与 200 ms 比较。
- 交互只覆盖这两个本地同步反馈；没有把 Lighthouse 分数或两次 rAF 耗时称为 INP。生产 RUM INP p75 尚未测量。

## 环境与结果限制

本机 WSL2 Linux 6.18.40.1、AMD Ryzen 7 7700X（16 logical CPU）、15.217 GiB 内存，cgroup CPU/memory 无额外上限；Node 22.22.1。与计划 Linux 8 vCPU/16 GiB/SSD 的后端参考环境不同，不能据此前端子实验声明后端容量或生产性能达标。操作系统缓存/主机负载不受此脚本完全控制；HTTP 浏览器缓存按上述条件清除。

首次三份报告未形成冷加载样本，原因是首页 h1 中换行使 Playwright 的 exact accessible name 比较等待失败。页面实际已加载，诊断记录标题及资产路径；修正定位器为实际首页名称后重跑。保留这些测量失败原始 JSON，并明确它们没有有效性能结果，不将其当作阈值通过。

2026-10-08 的首个完整报告记录首页 LCP 中位数 1604 ms、CLS 0、JS gzip 152,749 bytes；登录 1616 ms、CLS 0、JS gzip 155,428 bytes。随后对交互重复样本补充每次 reload、固定版本拒绝校验，再执行完整测量；最终数值以对应最新 JSON 原始报告为准。当前首屏体积在文档限制内，因此没有未经测量依据的分包改动。

最终方法复测报告：[frontend-2026-10-08T08-04-18-937Z.json](frontend-2026-10-08T08-04-18-937Z.json)。

| 测项 | 实测 | 文档阈值 | 此子实验 |
|---|---:|---:|---|
| 首页 LCP 中位数 | 1624 ms | ≤2500 ms | 达到 |
| 登录 LCP 中位数 | 1632 ms | ≤2500 ms | 达到 |
| 首页 / 登录 CLS 中位数 | 0 / 0 | ≤0.1 | 达到 |
| 首页首屏 JS gzip | 149.17 KiB | ≤300 KiB | 达到 |
| 登录首屏 JS gzip | 151.79 KiB | ≤200 KiB | 达到 |
| 本地显隐 / 验证反馈最大值 | 30.8 / 31.1 ms | ≤100 ms | 达到 |
| 实验室显隐 / 验证反馈 p75 | 28.4 / 30.1 ms | ≤200 ms | 达到 |

脚本通过 ESLint 和 Node 语法检查，最终方法实际跑完十次冷加载、二十次交互。以上只说明当前构建、两个匿名页面及指定本地反馈；账号/管理业务交互、T21 后端容量、真实生产 INP 不在此结果内。
