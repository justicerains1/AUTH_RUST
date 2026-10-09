# 真实账号和管理页面交互实验

2026-10-09（Asia/Shanghai，原始报告使用UTC）。[代理冻结测量](product-interactions-2026-10-08T20-23-43-253Z.json)于20:23:43～20:26:20 UTC完成，实际退出0；六种场景各五个本地样本、五个实验室样本，共60项全部保留。受测源码SHA逐项与最终文件一致，报告记录真实后端harness、浏览器可执行文件、生产dist每个输出的摘要。没有重跑匿名页面LCP/CLS冷加载，也不修改T21任务状态。

| 真实场景 | 本地反馈最大ms | 实验室反馈p75 ms | 实验室完整终态p75 ms | Event Timing p75 ms |
| --- | ---: | ---: | ---: | ---: |
| 账号退出dialog打开 | 47.9 | 46.5 | 46.5 | 16 |
| 账号退出dialog取消 | 47.8 | 46.4 | 46.4 | 16 |
| 设备会话下一页 | 47.7 | 45.5 | 228.9 | 24 |
| 撤销另一设备确认 | 48.0 | 46.5 | 562.1 | 16 |
| 后台应用客户端导航 | 48.1 | 46.3 | 213.4 | 16 |
| 客户端空表单错误反馈 | 47.8 | 46.6 | 46.6 | 16 |

本地1×CPU/无网络节流的首次可见反馈取五样本最大值，文档阈值100ms；实验室4×CPU/指定网络的首次可见反馈取nearest-rank p75（五次排序第四项），阈值200ms，全部达到。异步终态等待真实请求及界面更新，表中的228.9/562.1/213.4ms完整保留，**没有对完整业务耗时声称200ms内**。这次Event Timing每场景五个样本均有真实事件组，但数值仍有8ms量化、16ms报告下限；不是生产RUM INP。

固定浏览器为Playwright1.63.0 `HeadlessChrome/153.0.8010.12`、390×844。实验室CDP4×CPU，下行200,000 bytes/s（1.6Mbps）、上行93,750 bytes/s（750Kbps）、latency150ms；本地对照1×且不施加网络节流。每模式新incognito context，禁service worker、清HTTP cache并禁用浏览器缓存，页面从production dist提供gzip，不用Vite开发页面。CDP只限制浏览器条件，真实API/PG/Redis仍在本机；WSL2 Ryzen7700X、16logical、15.217GiB与规定后端参考8vCPU机器不同。

`tests/performance/product-interactions.mjs` 提供专属5320页面服务和真实5321API代理，保留Cookie/CSRF/请求方法，无响应mock。复用t23 harness，但只有`T21_PRODUCT_INTERACTION_FIXTURE=1`才准备性能用户数据；普通可访问性模式没有新增会话资料。每模式专属用户在真实PasswordService验证密码后，经有效preauth与 `complete_password_login` 权威仓储事务创建26个真实会话（21分页背景、5独立撤销目标）。浏览器自己通过真实密码登录，未直接注入浏览器身份；最终本人列表27项。管理员经真实密码及数据库未消费恢复码强认证，完整标准10码集合，未禁用或清除限流。

每次测量在可见触发按钮上预装capture click监听与PerformanceObserver，Playwright实际触发trusted输入；使用同一页面timeOrigin的event.timeStamp/performance.now。先等待rAF中出现真正可见反馈谓词，再经过两次rAF回调记录，属于**绘制机会代理**，不是精确像素paint或INP。反馈谓词分别为实际dialog/消失、表格loading状态、确认按钮busy、后台aria-current导航及真实alert；不把已有hidden节点当新反馈。完整终态另从同一输入时间记录页面最终状态：第二页7行、撤销结束及dialog消失、客户端表格caption加载。真实API ResourceTiming.duration另列，不使用Node时间推算浏览器耗时。

Event Timing使用`durationThreshold:16`，按输入窗口捕捉pointerdown/up/click的非零interactionId事件，取事件组最大duration而不求和。observer异步交付后takeRecords，记录eventCount/interactionIds；若少于五个有效报告，精确EventTiming p75写null并明确censored，不填0，不丢弃快样本。最终这次60项均实际有事件组。首次反馈与完整终态样本都保留，分场景计算，不把快速dialog混入较慢请求降低p75。

业务证明同时执行：

- 五次打开/取消退出dialog、初始取消焦点正确，零logout请求，原`/me`200。
- 五次真实会话cursor GET200，第一页20/第二页7，API IDs不交叉且DOM行数准确，不记录原cursor/身份ID。
- 五个不同有效非当前会话：撤销前真实`/me`200，显式确认DELETE204与列表更新，目标会话后401、当前会话仍200；第一页面显示规模保持20行，不通过反复删除同一目标或创建假401测量。
- 五次真实后台客户端GET200和表格加载，五次空表单真实本地alert。此场景只测导航与无效表单反馈，没有测成功创建/轮换端到端性能；它们的权限/键盘正确性由产品12例覆盖。

运行命令：

```sh
PATH=/root/.cargo/bin:$PATH node tests/performance/product-interactions.mjs
```

必须先有当前production dist、真实本地测试PG/Redis与私有开发签名文件；脚本生成本次随机密码/AEAD/控制键，0700目录/0600文件，正常结束清schema、服务器和私有材料，不停止共享开发依赖。五个目标session token只用于私有控制资料和测试请求，公开证据无token/Cookie/password/OTP/seed。最终TS、定向ESLint、harness Clippy与diff检查退出0；已接入 `npm run test:interactions`，完整 `npm run test:full` 在 `build` 后必跑该场景；完整编排现在十二阶段（含 npm ci）。

失败历史完整保留：[首次60样本报告](product-interactions-2026-10-08T20-10-57-264Z-failure.json)错误等待`.pagination [aria-busy]`，实际busy是主表Status，因此分页反馈被记成网络后完成212.3ms并退出1；源码确认后修为真正的`.status[aria-busy]`，没有改阈值、数据量或统计，未修改生产页面。[方法修正报告](product-interactions-2026-10-08T20-17-48-201Z.json)本地/实验室达到阈值，但harness准备期间移除测试callback未用解构导致启动前源摘要与最终spec不一致，已在该报告明确sourceReview；不把它冒成最终冻结源码。随后增加结束前源码SHA一致性校验，完整重放产生上述最新报告0。未删除慢样本或网络耗时。

边界：此实验补匿名页面以外六类账号/后台关键交互的本地反馈与实验室p75，不能证明全部管理过滤/详情、成功高风险写入、TOTP/Passkey、OAuth授权撤销、实体设备或生产RUM；Event Timing与rAF代理方法不同，各自范围明确。前端首屏/LCP/CLS仍依据原[冷加载方法](frontend-measurement.md)，生产INP、真实参考硬件、生产规模与正式发布关卡继续待验收。

## 根整合后实际复测

[2026-10-09T00:09:59Z 复测](product-interactions-2026-10-09T00-09-59-799Z.json)退出0、全部60样本保留。本地反馈最大47.9ms，实验室逐场p75最大47.2ms；实验室分页/撤销/导航完整终态p75为228.1/562.5/213.5ms，不作200ms业务完成保证。该次已包含管理员批量查询实现。原始表格保留对应代理冻结场次，不混合不同运行统计。

根整合将 Cargo 输出改为 compiler-artifact JSON，从实际本次测试目标读取唯一 executable 并计算 SHA，避免遍历 target 目录选中旧二进制。该受测 harness SHA为 `55deec27d1604856709dbb760bbbae54656f33a89a15ad723593497be47bcc4e`；77项页面/测试输入仍做运行前后摘要一致检查。新增根入口及完整编排三项行为测试、TS与定向ESLint退出0，T14真实权限/CLI回归退出0。尚未执行包含此次变更的最终全量回归，不沿用旧全量结论。
