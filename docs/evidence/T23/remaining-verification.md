# T23 第一版发布剩余验收

2026-10-08 UTC。此清单依据 [plan.md](../../../plan.md) 的 T23.01～T23.09、第 11 节外部输入、第 13 节完成定义，以及 [acceptance.md](../../../acceptance.md) 的 T23、浏览器表、E01～E28 和发布关卡。它仅整理真实证据与未完成条件，不修改任务结论或放行关卡。

文档要求第一版 T00～T23 全部必要验收通过。当前真实自动化与本地制品已有大量证据，但实体设备、完整发布回归、性能及生产操作仍不能推定完成。

## 按发布步骤核对

| 条款 | 已有证据 | 发布仍需实际验证 |
|---|---|---|
| T23.01 干净 checkout | 锁文件、分模块本地构建与 [T22 镜像构建](../T22/artifacts-summary.md) 已执行。 | 从明确提交的干净 checkout 安装、构建和测试，不依赖开发机缓存秘密、已有 target/dist 或未提交文件；保存提交、工具版本、安装和退出码。开发目录连续成功不能代替这项。 |
| T23.02 完整检查 | 已执行的真实模块和当日跨模块/故障/扫描见 [T20 汇总](../T20/cross-module-summary.md)。 | 发布版本整套 `npm ci`、check、unit、integration、e2e、security、accessibility、build 必须真实完成，必要测试无 skipped；保存整套汇总及失败/修复/复测。已有历史模块报告和本地结果不能自动代替该提交的远端 CI 成功。 |
| T23.03 五类浏览器/设备 | Chromium 自动流程、四宽度 axe、虚拟 Passkey 见 [T09](../T09/test-boundaries.md)、[T17](../T17/test-boundaries.md)、[T18](../T18/test-boundaries.md)、[T19](../T19/test-boundaries.md)。 | 手测当前稳定 Chrome、Firefox、macOS Safari、iOS Safari、Android Chrome 全部主要流程，填写真实版本；实体手机/桌面 Passkey 至少一组注册/登录/取消/密码回退，并完成人工交互检查。 |
| T23.04 生产行为 | 本地弱配置/测试数据库拒绝和真实页面/API 检查已有模块证据。 | 在最终生产制品核对调试/seed 禁用、最终品牌及合法文本、所有按钮/邮件链接/回调/错误页确实可用；生产域名下的 issuer/RP/Cookie/headers 与部署匹配。 |
| T23.05 外部运维条件 | [T22 artifacts-summary.md](../T22/artifacts-summary.md) 记录本地镜像/config 验证；[single-host-deployment.md](../../runbooks/single-host-deployment.md) 为操作稿。 | 正式域名/DNS、Linux 生产部署、真实 SMTP DNS 与送达、独立加密备份/WAL、新主机恢复、密钥轮换和可达告警均要实际执行并留证。操作稿不是演练结果。 |
| T23.06 安全风险 | [当前安全六阶段](../T20/security-checks-2026-10-08T09-40-39-229Z.json) 退出 0；[有限 ZAP](../T20/zap/scan-1791452463058.json) 范围内 High/Critical/Medium/Low 为 0。 | 发布版本扫描与威胁/并发完整回归、信息及误报分诊、未解决风险影响和日期；关键场景未通过仍禁止发布。有限 ZAP 不覆盖全部写接口/业务，也不是 OpenID 官方认证。 |
| T23.07 版本制品 | T22 已记录本地生产镜像 digest。 | 为最终发布版本生成可复查版本、制品 checksum/digest、变更说明、部署与兼容 rollback 指令；真实验证迁移失败停止发布和保留上版镜像。不能使用仅供本地 review 的 tag/digest冒充正式版本交付。 |
| T23.08 生产冒烟 | 本地真实注册/MFA/虚拟 Passkey/双 BFF/撤销/管理员模块已执行。 | 正式部署后按完整链路实测：注册→真实邮件验证→密码/MFA/实体 Passkey→A/B SSO→全部退出→管理员禁用后旧凭证/刷新拒绝。 |
| T23.09 上线观察 | 尚无生产观察记录。 | 至少 24 小时及一个实际业务高峰，记录认证、撤销、邮件、队列、数据库/Redis、错误和资源；发生异常按 runbook 回滚且保留原始证据。等待时间不能被本地短测替代。 |

## 浏览器与人工交互边界

现有主要自动化为 Linux headless Chromium。前端性能报告固定 `HeadlessChrome/153.0.8010.12`，这是具体测量工具版本，不是五类浏览器均已手测通过的证明。

| 平台 | 文档要求 | 当前证据边界 |
|---|---|---|
| Windows/Linux | 当前稳定 Chrome 完整主要流程与实测版本 | Chromium 真实自动化已执行；仍需最终发布版本人工矩阵。 |
| Windows/Linux | 当前稳定 Firefox | 未有完整真实流程证据。 |
| macOS | 当前稳定 Safari | 未有完整真实流程与实体认证器证据。 |
| iOS | 当前稳定 Safari | 未有手机完整注册/恢复/MFA/Passkey/SSO/返回布局证据。 |
| Android | 当前稳定 Chrome | 未有手机完整注册/恢复/MFA/Passkey/SSO/返回布局证据。 |

T09-PK-04 仍为实体设备待验收，不能把 CDP 虚拟 CTAP2/真实签名自动化改成硬件通过。自动 axe、有限键盘和 reduced-motion 检查也不能替代屏幕阅读器、焦点与错误播报、对比度、200% 放大、密码管理器、复制粘贴、系统返回、取消认证器与实体手机手工检查。

## T21 性能的真实进展与未完成部分

[前端方法及结果](../T21/frontend-measurement.md) 已实际完成：首页/登录各五次冷加载，固定 390×844、4×CPU、1.6 Mbps/750 Kbps/150 ms。最新方法报告 [frontend-2026-10-08T08-04-18-937Z.json](../T21/frontend-2026-10-08T08-04-18-937Z.json) 的两个匿名页面 LCP/CLS、首屏 gzip 和指定同步反馈达到相应实验阈值。

该结果只覆盖两个匿名页面及显隐/输入验证反馈；没有把两次 animation frame 当作生产 INP。账号/管理关键交互及样本充足后的生产 RUM INP p75≤200 ms 仍需记录。

[后端方法](../T21/load-method.md) 已明确真实十万用户/授权、20 客户端、512 个真实 code-exchange 令牌家族和受控 refresh，Argon2 64 MiB/t=3/p=1、不降安全参数。首个完整 introspection 报告 [load-introspection-2026-10-08T09-24-01-720Z-failure.json](../T21/load-introspection-2026-10-08T09-24-01-720Z-failure.json) 保留：120 秒预热+900 秒测量，269441 请求/299.38 RPS，业务错误/429/令牌更新失败为 0，p95=4 ms/p99=5 ms，但 dropped iterations=560、请求数门槛未达，**退出 1，不能填写达标**。后续复测以新原始报告为准，不删失败样本。

尚需完整成功的 introspection 300 RPS、account 100 RPS、password 5 RPS 和 mixed 300+50+5 RPS，分别两分钟预热/十五分钟测量，资源、连接池/哈希队列、热 SQL、错误分类和容量边界都要保存。当前 WSL2 Ryzen 7700X 16 logical CPU/15.217 GiB 与参考 Linux 8 vCPU/16 GiB/SSD 不同；即使本机子实验达到指标，也须准确说明环境，不能推定参考环境或生产容量达标。

## T22 生产与恢复条件

已真实验证 Rust release 与三前端 production 镜像构建，runtime UID10001/read-only、不可写 `/app`、二进制/系统库版本，Caddy 合成域名 validate，Compose 派生副本 config。没有实际 `production up`，合成 example 域名校验不证明真实证书成功。

以下仍缺真实验收证据：

- 干净 Linux 生产主机部署、正式域名/DNS、固定 issuer/RP、安全 Cookie/HTTPS/HSTS/CSP；只开放规定端口、DB/Redis 不外露、SSH 受控。
- 配置验证→备份成功→迁移→启动→readiness→业务冒烟的完整发布与兼容 rollback 实操；迁移失败停止发布。
- 真实 SMTP 账号与 SPF/DKIM/DMARC，实际验证/恢复/通知邮件送达；Mailpit 投递和故障重试不替代生产送达。
- 等价监控系统及 5xx/延迟/队列/连接池/攻击/磁盘/证书/备份告警，实际可控故障触发且告警可达。
- 每日基础备份、持续 WAL 到独立加密存储、至少 14 天保留；数据库/配置元信息及另受控签名/AEAD 密钥备份。
- 新主机恢复到已知时间点，实际验证密码/TOTP/Passkey/OAuth/撤销；完整计时并核算 RPO≤15 分钟、RTO≤60 分钟。
- 签名新公钥先发布再切签、旧公钥保留兼容窗口；AEAD 旧 key 解密/版本化重加密，真实轮换后 TOTP 和现有凭证不丢。

正式域名/DNS、Linux 主机入口、SMTP、独立存储、真实设备及正式品牌/合法文本的要求来自 plan.md 第 11 节。可继续本地实现和验证，但外部条件未验收不能降低原关卡。

## E01～E28 最终回归要求

总表共 **28 个场景**，威胁矩阵 TH01～TH34 是另一组编号。下表只标现有证据来源及剩余边界，不给 E 场景写最终“通过”。

| 场景 | 已有真实模块来源 | 最终尚需核对 |
|---|---|---|
| E01/E02/E03/E04 | T04～T07、T10、T17 注册/响应统一/限流/会话轮换 | 发布版本完整回归与真实邮件。 |
| E05/E06/E07/E08 | T07/T08/T18 MFA 限定挑战、同 step/恢复码单次消费、重置保留因素 | 发布版本端点/浏览器完整回归，不借 fixture 冒因素成功。 |
| E09/E10 | T09 虚拟认证器签名及 UV/origin/challenge 攻击 | E09 实体 Passkey 注册/登录未验收；E10 发布版本负向回归。 |
| E11/E12/E13/E14/E15/E16 | T10～T14/T20 真实协议绑定、code/refresh/撤销/禁用及 A/B | 发布版本整套及生产双应用冒烟；互操作不冒充官方认证。 |
| E17 | T14/T19 普通用户真实 API 403 | 最终制品后端权限与 UI 完整回归。 |
| E18/E19 | T04/T05/T12 当日真实依赖停机；T13 BFF 状态失败关闭 | 发布版本完整结果；生产 SMTP/告警另需 T22。 |
| E20/E21 | T22 制品和操作稿、T04 算法兼容边界 | 独立主机备份恢复与实际签名/AEAD 轮换尚未演练。 |
| E22/E23 | T16～T19 自动 axe/键盘/reduced-motion 和移动宽度 | 人工读屏/交互、实体手机和五类浏览器完整流程未验收。 |
| E24/E25/E26/E27 | T05/T07/T12/T14/T20 GET 不消费、真实竞争、跨客户端/末位管理员保护 | 发布版本完整回归和修复后证据对应。 |
| E28 | T01/T03 本地弱配置/seed 拒绝；T22 制品验证 | 最终生产构建与部署配置的实际拒绝检查。 |

T24 为后续高可用阶段，不阻塞第一版；第一版的单机维护中断须如实说明。没有 T24 多副本/数据切换/SLO 独立实测，不能承诺 HA 或 99.9% 可用性。

放行仍依据原 acceptance.md：关键认证/撤销/权限失败、High/Critical、恢复不可用、邮件不送达、生产不安全、关键浏览器阻塞或必要测试未执行，均禁止第一版发布。完成本清单的编写不等于用户要求的整个系统已交付。
