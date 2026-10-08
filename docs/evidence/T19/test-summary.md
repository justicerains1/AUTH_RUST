# T19 管理后台产品与真实大数据分页验收

2026-10-08，父ede1752，Linux/Rust1.98/Node22、PG17/Redis7.4、Chromium153。T14/T16前置通过，三项必要UI案例真实通过，代码版本由本模块Git提交记录。

四个真实后台页面提供用户查询/详情/启用禁用/会话撤销、客户端创建编辑停用/轮换秘密、管理员授予移除/末位保护、固定审计窗口与分页。每个请求后端强认证及权限，不靠UI隐藏；普通用户后台页面无数据且实际API403。

高风险单流程先影响说明→当前身份真实验证→再次明确确认，不自动危险POST、不叠两个modal。客户端secret只成功结果局部state/ref展示，关闭移除DOM与引用、不进Query/Mutation cache。真实创建与轮换/关闭secret通过，恶意名称仅React文本，不能生成img或执行事件。

测试identity_test随机schema实际100000用户，用合法PG UUID、真实密码摘要fixture；列表每页20，cursor真实HTTP200，完整邮箱/status精确filter及31天审计限制。0008加入固定排序/状态分页索引，SQLx build.rs目录变动追踪确保新增迁移被嵌入；ANALYZE后EXPLAIN ANALYZE BUFFERS实际Limit20+users_created_id_idx IndexScan，Execution Time0.051ms（单次本机值，不当容量SLO）。原SeqScan13.728ms计划/迁移缓存失败保留。

真实3E2E0；check/unit/build/docs/OpenAPI与65工具通过，本次身份58+演示12测试。T06/T07/T08/T09/T10/T12/T13受UI变动旧E2E全部回归0，原案例不删，步骤更新使用新明确认证dialog。T09实体设备仍待验收，不影响T19自己T14/T16前置；T20/发布设备依赖保留。

失败修复：固定时钟提前strong_at被真实browserguard拒，改真实SystemClock与独立单次恢复码重认证；fixtureMD5 UUID variant非法导致严格schema拒，改PG合法UUID而不放宽；秘密DOM与按钮行定位准确；索引嵌入缓存加buildscript并保留旧计划。来源结果见e2e.txt/pagination-plan.json/旧计划/ui-source-summary.md及最终命令记录。
