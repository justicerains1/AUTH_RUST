# T20 安全、协议、并发与故障模块记录

2026-10-08，父10067a5。代码检查和已执行安全子套件通过；T18前置依实体设备待验收，完整模块与发布保持待验收，最终全量回归仍待长压测释放依赖后实际执行。

实际六阶段security命令退出0：RustSec缓存366依赖0漏洞、cargo-deny全第三方许可/来源0、在线npm audit0、完整Git历史Gitleaks精准分诊后0、新跨模块HTTP四窗口0、生产构建匿名/已登录ZAP复测0。ZAP自身/api/v1/me匿名401/登录200已核实，spider及仅GET/meAPI主动扫描完成，最新High/Critical/Medium/Low0，Informational保留；不当全接口攻击或OpenID官方认证。

真实故障T04/T05/T12本次顺序运行，PG/Redis503关闭、SMTPoutbox保留并恢复重试，三依赖最终healthy。新跨模块刷新已Basic认证但等待时secret轮换不能mint；codeuserdisabled拒，dupcritical/CORS拒，RP准备后回调注册改变确认拒。其余威胁/并发/密码/因素/撤销已执行模块映射见cross-module-summary.md，最终整套manifest另执行。

ZAP初轮Vite开发响应缺头中低告警保留；换正确productiondist及同Caddy固定头，并实际补API CSP form-action后复测无中低。测试脚本参数大小写/URL预载错误保留。ESLint原扫入.local下载ZAP第三方脚本失败，修为只排除已被Git忽略的私有工具目录，不关闭源码规则。

另真实Worker投递audit target_type=outbox在严格管理员schema遗漏，补API及DTO枚举；4项UI单元含outbox可读，合成DTO未冒充SMTP到后台完整联动。最终rootcheck/unit/build及docs/OpenAPI/工具通过，完整test:integration/E2Emanifest逐module失败继续记录私有诊断；缺脚本必失败。正式test:full待执行，不能仅runner存在写全套通过。

生产TLS/真实SMTP/独立恢复/设备/屏幕阅读器以及T21首次容量调度失败尚未放行。目录内失败和原报告保留，证据界限见ZAPtriage、cross-module-summary与T23 remaining-verification。
