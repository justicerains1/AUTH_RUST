# T22 本地部署编排与兼容回滚

2026-10-09（Asia/Shanghai；以下UTC）。新增[release.mjs](../../../infra/ops/release.mjs)、[编排单测](../../../tests/ops/release-order.test.mjs)和[真实独立演练](../../../tests/ops/release-local.mjs)，runbook已补受控配置/命令。没有生产域名/SMTP/独立存储输入，没有向生产执行发布。基线使用已有本地`auth-rust-runtime:t22-review`与`t23-1c0c9ea`镜像，不声称新版本源码重建/跨任意schema兼容；edge只核对制品存在，演练选择API服务并明确local-review授权。

最新真实`node tests/ops/release-local.mjs`于16:49:03.700～16:49:57.975 UTC退出0，[原报告](release-local-1791478197976.json)源码SHA与当前release.mjs匹配。任务专有Compose项目内网PG/Redis、无任何主机端口，Node冒烟容器在同一内网访问实际API；不停止或更改开发PG/Redis、不给其他压力测试施加载荷。

- 实际CLI通过stdin创建已验证但尚binding-only的测试账号；smoke验证其真实密码/session/me/退出权限，不冒管理员全权限。固定非root/read-only/cap-drop/no-new-privileges应用容器启动。
- old与new两次发布均执行真实配置校验、PG迁移/在用AEAD kid查询、实际pg_basebackup/pg_verifybackup/age加密、回执/manifest/密文大小和SHA验证，再迁移成功、up、健康、实际密码登录/me200/退出204/旧me401。记录旧/新镜像引用和每阶段结果，新备份必须来自本次操作；零退出旧回执不能冒成功。
- 专属SQLx迁移记录checksum受控破坏，真正运行identity-migrate失败；编排未执行up/ready/smoke，当前API容器ID与成功new记录保持。恢复此测试记录的原checksum后，实际history摘要回到原值；不对真实生产历史做这种准备。
- rollback比对受控记录的旧镜像、允许migration摘要、旧版支持kid与实际当前DB/key版本，再验证新备份、切旧API、readiness及真实smoke成功。不运行迁移/downgrade，不删除数据卷。兼容例子只证明本次历史镜像及相同schema/key配置，不能推定任意版本兼容。

`node --test tests/ops/release-order.test.mjs`5通过/0失败/0skip，覆盖新备份→迁移→up→ready→smoke、迁移失败中止/成功状态不变、旧回执及损坏密文拒绝、缺或错schema/key兼容拒绝、实际facts超出旧schema或kid范围在up前拒绝、production浮动tag/未明确授权拒绝。这里控制child只证明编排，另有上述真实容器证据；定向ESLint与diff-check均0。

早期实际失败报告保留`release-local-*.json`：未传Compose env导致缺image、tmpfs未加引号被YAML拆项导致Docker拒绝挂载（该问题同时存在生产Compose，根已最小修复两处）、internal-only网络没有主机端口发布导致host smoke ECONNREFUSED，改为同内网Node真实请求且无host端口。没有去掉read-only/cap-drop限制或把health代替业务smoke。后续调试失败与成功日志分别保存，不删除失败证据。

运行时配置/状态/回执/诊断受控0700/0600，public report只阶段、镜像引用、backup ID/密文SHA和迁移摘要；密码/stdin、私钥、Cookie和token不写报告。专属资源结束清理只由演练脚本处理；正式release脚本不执行down、drop或volume删除。production CLI必须明确`--allow-production`，仅不可变image，真实部署前仍须原T20/T21/外部关卡通过。

正式Linux主机/TLS、真实SMTP/DNS、独立故障域备份/WAL和14天保留、所有实际业务冒烟及生产新旧schema/key兼容、至少24小时高峰观察仍待真实验收。该脚本与本地演练使发布顺序可执行、可复查，不自动完成T22/T23生产放行。
