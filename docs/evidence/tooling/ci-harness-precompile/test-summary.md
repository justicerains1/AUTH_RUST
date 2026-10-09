# CI产品harness启动前预编译

远端fef4156的product组首次直接运行T17时出现harness failed to start；该组环境/镜像构建已通过。原runner从cargo test启动开始计60秒，包含冷编译，没有单独预编译步骤。当前没有详细远端编译尾日志，因此冷编译是可验证的计时缺口而不是已证实的唯一失败根因。

CI新增按组cargo test --locked --no-run，编译先完成再运行原案例；保留T17原60秒启动限制和所有断言，未关闭认证/放宽限流。product涵盖t17/t18/t19ui/t19management/t23accessibility，authentication另按真实crate预编译t03_database数据库测试及server相关target，protocol保留原五target。编译错误独立阶段脱敏输出且非零中止。

本地product五程序实际预编译0，75工具测试、actionlint、ESLint及docs均0；产品完整重测已退出0：T17六案例、T18四案例、T19原三+新四案例，以及T16/双浏览器12产品可访问性均通过。新远端结果仍待验证，不先称已修复所有启动故障。

普通CI新增push/pull_request paths-ignore：仅docs/**或Markdown变更不触发构建；应用/测试/依赖/workflow/安装脚本变更仍触发。混合代码与文档提交仍运行完整CI，恢复发布workflow仅手动触发不受此规则影响。actionlint验证0；新远端运行与实际过滤由GitHub随后确认。
