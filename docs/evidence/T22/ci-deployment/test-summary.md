# CI成品与生产无构建部署

新增CI发布job依赖现有Linux/Windowscheck-and-build和真实integration任务全部成功，main push才执行。CI构建locked Rust runtime与三前端/Caddy edge并推GHCR，镜像用完整SHA tag及OCI digest；生成release-manifest逐文件SHA和部署包，Actions artifact保留30天，同时在固定ci-SHA GitHub预发布供下载。包不含crates/apps/node_modules或身份密钥，包含已官方摘要核验的age工具。

生产入口deploy-production.sh调用Node编排，仅Docker/Compose+Node运行，不npm安装/不Cargo/Vite编译。实际包checksum先验证、Compose任何build字段拒绝、所有镜像必须digest且应用镜像必须与CImanifest一致；pull失败不启动，所有Compose up/run加--no-build。默认五应用配置、真实DB版本事实、独立新加密备份/校验、迁移、readiness、业务smoke沿用现有release流程。回滚保留兼容审查、不降库、不删除数据。

本地实际包生成/归档SHA验证完成；[十一测试](tests.txt)全部0/skip0（新增5项加原6顺序测试），覆盖禁止build/浮动digest/不匹配，plan无mutation，pull失败中止，真实文件receipt验证后migration失败不up应用。production-config原模型约束不变。定向ESLint、Node/shell语法、25任务/199步骤文档与OpenAPI检查均0；actionlint1.7.12正式二进制摘要核对后workflow校验0，shellcheck未安装不冒已执行。

[结果和源SHA](result.json)已保存。没有用模拟executor证明生产部署；真实GHCR push/GitHub release下载需新workflow在远端运行，当前未宣称成功。生产域名/主机/SMTP/独立存储/业务smoke配置仍需准备，脚本不自动生成生产秘密或替代原验收关卡。

操作、下载路径和首次准备见[生产CI部署说明](../../../runbooks/production-ci-deployment.md)。T22入口新增无构建部署必跑测试，现有真实五服务发布/回滚与备份测试保留。
