# 本地最终范围复核：可访问性、秘密显示与安全头

审查于2026-10-09（Asia/Shanghai），源码起点 `c3885fa566597dff81424b5ffdf63eab55669377`。本轮只读源码、测试及公开证据，不运行新浏览器/安全/容器测试、不修改产品。根代理正在执行新完整回归（会话87701）；本文不预判其结果。此前bc742d1全0仅是历史完整回归；当前[新镜像证据](final-artifacts-20261009.md)已完成，runtime为2c9c712、edge为72dbe6a，分别有源码等价/构建摘要，不能继续用1c0c9ea镜像代表本次生产源码。

后续状态补注：本文识别的两个本地焦点缺验证已按根授权追加真实用例，创建secret关闭失焦在两浏览器复现并最小修复；Action→密码/恢复码→Action连续键盘焦点及秘密关闭返回均实际通过。最新[两浏览器12例完整报告](product-accessibility/2026-10-08T19-04-12-019Z.json)退出0，详[更新总结](product-accessibility/test-summary.md)。下方缺验证列表保留为审查时点的历史发现，不能作为当前仍未处理项；下载正文比对/外部人工边界仍保留。全量87701最终仅T05集成失败，其余阶段包含当时原10例均0，新焦点修复之后的最新全量和新edge制品仍须根代理重新安排。

可访问性已有真实覆盖：[产品双浏览器](product-accessibility/test-summary.md)为Chromium153与Linux Playwright Firefox155的10用例，80组四宽账号/后台布局、20组200%文本缩放、真实注册/邮件/密码/重置键盘操作，管理员实际恢复码登录、独立身份确认错误与取消、AdminAction取消焦点返回和无mutation。T16组件与T17公开页面另有键盘/axe/reduced-motion证据。这些可以证明受测路径，不能写成Firefox全套MFA/Passkey/SSO、所有成功危险操作的完整键盘验收。

具体仍缺的**本地验证**，与外部设备分开：

- `ClientsPage`的“保存客户端秘密”dialog明确将初始焦点放在关闭按钮，秘密关闭后setSecret(null)，但没有 `Dialog.Trigger`或显式`onCloseAutoFocus`；T19实测创建/轮换/DOM清除，没有断言关闭后焦点回到开启按钮。不能据AdminAction取消结果推断这一路已验收。
- AdminAction“确认身份后继续”会关闭原dialog并打开ReauthenticationDialog；后者捕获当前activeElement作为opener，完成后再开AdminAction。现有T19/单元验证认证→显式确认→mutation顺序，但没有真实断言整个连续切换过程中初始/约束/返回焦点。可能来自即将卸载按钮的opener必须实际复现才能判缺陷；本审查没有运行，所以只记缺验证，不声称已失败。
- 现有恢复码下载E2E确认download事件/文件名，没有读取下载内容逐码比较；unit检查显式点击及URL回收。秘密copy/display测试证明有限API/DOM/查询缓存行为，不证明浏览器内存已安全擦除，也不证明全部失败取消路径的键盘体验。

真实秘密复制/显示证据应如实保留：T18真实后端重建恢复码之后，浏览器授权剪贴板能力、实际复制并读取10行、实际download事件、关闭region清除；不能称这部分只有mock或全部未验。T19真实创建/轮换客户端秘密，首次显示/关闭后DOM不可读，单元另检查秘密不进入Query/Mutation缓存与localStorage/sessionStorage。组件说明下载文件为本机明文、保存到可信位置、共享设备剪贴板需清理；复制失败有手动选择/下载回退。当前10case产品无实际恢复码生成/复制/下载路径，Firefox该路径仍未在这套逐browser矩阵验证。

**外部/人工验收**仍明确：真实屏幕阅读器NVDA/VoiceOver等错误与状态播报、密码管理器、系统返回/剪贴板设备体验、完整浏览器200%zoom、实体手机/iOS Safari/Android Chrome、macOS Safari及实体Passkey。32px根字号是文本缩放，axe `color-contrast` incomplete要人工核对；ARIA/自动焦点不等于人工读屏通过。这些必要条件缺少不能把E22/E23或T23.03标全通过。

新镜像安全头覆盖来自实际Caddy，而不是只看Node测试服务器：三个站点使用原Caddy路由、受控测试CA/主机名TLS、HTML/JS内容SHA，对HTML no-store、资产一年immutable、CSP/HSTS/nosniff与成功响应Server移除逐项验证。缓存缺陷由真实响应发现并在72dbe6a拆分`header -Server`修复，认证HTML/API no-store未放宽；缺后端API/OAuth/BFF502仍no-store。Caddy自生成502的Server指纹为已记录低信息行为，不能用静态成功头结论推定所有错误页都移除Server。生产域名CA/ACME、真实后台proxy请求全部头部、实际部署冒烟仍未执行；`production-site.mjs`的CSP/缓存代理测试不代替Caddy实测。

T21/T22/T23仍有需要区分的范围：本机新容量/共享Redis900秒长测与SQL记录已补局部证据，规定参考硬件/生产RUM/实际高峰未提供；新镜像已存在，最新全量还在跑；T22受控主机collector/告警规则与backup完成记录存在，但实际systemd等每日调度、WAL新鲜度/归档延迟接入和保留链执行不能只归为“等生产资源”，需要具体交付/验证与真实独立存储部署分别记录。根代理已委托保留方案审查，本文不新增方案或删除备份。生产SMTP/DNS/通知送达、新主机完整规模恢复/RPO-RTO与24小时观察仍是真实外部验收。

状态同步建议：acceptance与remaining-verification/e2e矩阵中的旧“T18/T19只有点击fill、仅9告警、小probe、签名分发完全未做、最终镜像1c0c9ea”应在新完整结果后更新为当前实际局部范围；保留人工/实体/参考/生产关卡。此报告没有直接改任务状态，未扩大功能或推定T24前置已满足。
