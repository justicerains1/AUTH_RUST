# CI执行耗时优化

依据GitHub实际历史run37888996667 step时间，npmci2～7秒/工具测试3～6秒；耗时主要为Rust编译和串行集成/浏览器。Windowsrelease build492秒、unit270秒、check240秒；原integration全部命令约1283秒，其中按新组映射authentication558秒/protocol235秒/product490秒，尚未计环境准备。原成功时间见[实际记录](baseline-timings.json)，不以历史映射预报实际新pipeline速度。

改动：同分支新push取消过时普通CI；Linux/WindowsRust注册表及target缓存按OS/toolchain/lock隔离；集成分认证/协议/产品3个独立runner，各自PG/Redis/邮件和浏览器，不在共享实例并发故障。组内原命令序列保留，每条原CI命令恰好一次，fail-fast=false让其他组继续；成品发布依赖所有矩阵job成功。

development与production镜像使用固定SHA的docker build-push-action和GHA缓存，开发镜像一次构建给四backend/三个webtag，compose启动--no-build避免重复构建。缓存scope按组隔离避免同时写同键；这些层缓存不保证Cargo cache mount每次完整持久，源码变更仍可能触发编译。生产仍CI构建，服务器只拉成品。Release步骤用RELEASE_TOKEN（回退github.token）同步此前已确认的Workflows权限修复，不更改registrytoken。

73工具测试/3分组覆盖与失败行为/actionlint/ESLint/文档OpenAPI/diff均0。新protocolgroup已真实完整运行0并保留T10～T14实际结果。[验证](validation.json)。未降低Argon2/队列或浏览器验收阈值、未删除测试。第一次GHA/Rust缓存为空，实际提速需远端新run和后续warmcache比较；不能声称当前已有确定分钟数收益。
