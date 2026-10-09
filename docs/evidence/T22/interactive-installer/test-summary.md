# 单文件交互安装入口

新增根目录install.sh，支持sudo交互执行的Debian/Ubuntu x86_64。用户只需下载一个shell文件；脚本验证Docker仓库签名key，安装Docker/Compose运行依赖，下载固定Node22.22.1并核对官方SHA，查找已经完成的CI部署release，下载包及校验和、拒tar路径穿越/链接/特殊文件，核对manifest后安装部署工具。生产不运行Cargo、npm或Docker build。

交互向导填写三站域名/TLS邮箱/SMTP/独立挂载备份/age公开recipient/管理员账号密码；随机生成数据库、RSA3072、AEAD、指标和BFF秘密，0600与对应UID。实际配置校验、新基础备份才迁移，真实管理员CLI与HTTPS密码确认/TOTP绑定后通过adminAPI创建BFF客户端。用户交互输入认证器验证码，恢复码写受限文件，成功后删除临时TOTP setup文件。每日备份timer由成功安装开启，旧数据/密钥/原配置不覆盖；部分失败拒无依据自动重试和重生成。

[15项本地测试](tests.txt)全部0，无skip。新增参数/环境注入拒绝、shell帮助及错误版本、Cookie删除/CSRF在MFA更新、实际专属PG17创建表后pg_basebackup/verify/age加密，再age解密并核backup_manifest；错误设备不更新receipt。实际测试发现pg_verifybackup成功消息污染密文stdout，改为stderr后解密验证通过；不只验证SHA就称合法加密。原5个CI部署测试和6个发布顺序测试同时保留。

定向ESLint、bash语法、actionlint以及文档/OpenAPI均0。未在真实新服务器安装系统软件、未签发生产ACME或发送真实SMTP、未跑完整管理员交互，因为生产外部条件尚未提供。CI首次成品发布仍须远端实际完成；脚本找不到完成release就停止，不能回退生产编译。当前自动smoke仅服务/HTTPS/issuer/Cookie和真实初始化结果，不冒全部注册邮件/实体Passkey/SSO/撤销/24小时验收已通过。

用户命令见[单文件安装说明](../../../runbooks/production-ci-deployment.md)。CI包包含install.sh与两个helper，固定SHA预发布附件也带install.sh。镜像/部署包尚未实际远端发布时，不能承诺下载即安装成功。
