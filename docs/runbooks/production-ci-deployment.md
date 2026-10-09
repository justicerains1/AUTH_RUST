# CI成品与生产一键部署

## 只下载一个脚本安装

全新Debian/Ubuntu x86_64服务器，以具备sudo权限的账号执行：

```sh
curl -fL https://raw.githubusercontent.com/justicerains1/AUTH_RUST/main/install.sh -o install.sh
sudo bash install.sh
```

先下载文件再运行，交互过程中读取终端，不使用`curl | bash`。脚本会提示安装Docker/Compose与固定校验的Node运行时；不会编译Rust、安装npm依赖或Docker build。选择最新已完成的CI部署release，也可以`sudo bash install.sh --release ci-<完整提交SHA>`固定版本。GitHub仓库/镜像私有时交互输入GitHub token（隐藏），token需仓库读权限与read:packages；不会记录token或作为命令参数传递。成品发布未完成时脚本停止，不回退源码构建。

交互填写安装路径、身份中心和A/B域名、TLS邮箱、SMTP主机/端口/用户名/密码/发件人、已挂载独立备份路径、age公开recipient、首管理员邮箱和隐藏密码。域名DNS和入站80/443须提前可用；WebAuthn不支持纯IP生产origin，向导拒绝IP，不用临时域名绕过接入要求。SMTP需要证书验证的STARTTLS。备份路径须是真实挂载点，age私钥由运维另行保管，安装器只收公开recipient。

向导检测Nginx：没有或未运行时使用CI成品中的Docker Caddy并自动申请HTTPS证书；若Nginx正在运行，则交互询问是否停止它并切换到Caddy。拒绝切换时保留Nginx并退出；端口冲突时停止安装，失败且端口已释放时尝试恢复Nginx。成功切换后询问是否禁用Nginx开机启动，原配置保留。域名全部由安装时输入，不写死CDNGOD域名。

向导生成RSA3072/AEAD/随机数据库和指标秘密，文件0600与应用UID匹配，首次加密备份成功后才迁移。真实CLI初始化管理员后，通过HTTPS密码确认、TOTP绑定（交互输入认证器验证码）取得强认证，再通过管理API创建两BFF客户端并写入秘密；不直接SQL造权限。完成后全部服务运行、开启每日02:00UTC备份timer，显示身份与A/B地址。恢复码在安装目录`.local/production/administrator-recovery-codes.txt`，请另可信保存；管理员密码不持久化。部分失败保留诊断和密钥，不自动重新生成或覆盖。

当前自动冒烟验证三站HTTPS安全头、API readiness、issuer和Secure/HttpOnly Cookie，管理员及客户端创建由真实请求验证；它不会自动声称真实SMTP送达、实体Passkey、完整浏览器SSO/撤销或24小时生产观察已经通过。首次生产验收仍按既有清单完成。再次运行脚本检测installed记录，可确认升级；保留数据、原配置与密钥，沿用备份/兼容发布流程。

生产主机只拉取成品镜像，不运行Cargo、npm、Vite或Docker build。`main` push的Linux/Windows检查与现有真实集成CI通过后，发布job构建linux/amd64 runtime和edge，推送GHCR；随后生成含不可变镜像digest的release-manifest、部署脚本、Compose、运维工具和已校验age可执行文件。前端静态资源和Rust二进制均在CI编译。

下载位置：GitHub Releases的`ci-<完整提交SHA>`预发布，或同次Actions的`production-deployment-<SHA>` artifact。包为`auth-rust-deploy-linux-amd64.tar.gz`及其`.sha256`。这类发布只是成品可下载，不代表生产验收通过。当前默认仅linux/amd64；ARM主机不使用此包。

生产主机前置：Linux x86_64、Docker Engine/Compose v2、Node22.22.1（运行部署编排，未安装npm依赖）、tar、sha256sum；具备Docker操作权限。私有GHCR镜像需提前`docker login ghcr.io`，使用仅read:packages权限的凭据，通过stdin输入，不放进命令参数或Git。公网域名/HTTPS/SMTP和独立备份需按单机runbook实际准备。

首次下载与校验（SHA取GitHub实际发布的版本，不用latest）：

```sh
sha256sum --check auth-rust-deploy-linux-amd64.tar.gz.sha256
mkdir -p /srv/auth-rust
tar -xzf auth-rust-deploy-linux-amd64.tar.gz -C /srv/auth-rust
```

先核对来源、发布版本与校验，再解压；包不含账号数据或任何秘密。内部manifest逐文件校验能发现文件修改，不代替发布方身份/下载来源验证。更新也下载固定SHA包，不在生产git pull源码或执行build。不要覆盖`.local/production`的秘密或释放旧版兼容配置；包只带公开age工具。

准备：复制`infra/production.env.example`为`infra/production.env`，填写真实三站域名、TLS联系邮箱、独立BACKUP_DESTINATION、WAL_ARCHIVE_DEVICE和AGE_RECIPIENT。镜像选择由manifest提供，脚本不采用手填浮动tag。按现有runbook准备`.local/production/identity.env`、A/B env和所有0600 secrets；独立wal目录须预建、UID999所有且设备号一致，备份私钥与身份密钥另受控备份。首次初始化/绑定管理员和注册BFF客户端仍按既有CLI流程完成，不自动生成未知生产账号或跳过因素。

复制`infra/ops/release-config.example.json`为`.local/production/deploy.json`，改为实际绝对路径并chmod0600。备份与业务smoke命令是已审查的本机受控程序；备份实际调用base-backup.sh并将结果写backupDirectory，smoke真实检查业务，不用仅health冒充。数据库facts由脚本直接查询实际Compose数据库，自动使用正在部署或回滚的镜像选择，无需另写facts命令。不得在这些受控程序中编译源码。

只读预览：

```sh
sh /srv/auth-rust/infra/ops/deploy-production.sh plan /srv/auth-rust/.local/production/deploy.json
```

配置准备后的一键发布：

```sh
sh /srv/auth-rust/infra/ops/deploy-production.sh deploy /srv/auth-rust/.local/production/deploy.json --allow-production
```

脚本先验证bundle摘要和Compose；发现任何build指令或非digest镜像立即拒绝。拉取CI镜像及固定PG/Redis镜像，启动数据依赖，验证四Rust应用配置、实际schema/key版本、新加密备份及摘要，然后独立迁移、以`--no-build`启动和wait、执行受控业务smoke。迁移失败不替换应用，成功写current和previous；失败留受限阶段诊断，不删除卷或自动降库。

保留前版成品及实际schema/key兼容证明，配置compatibilityFile后回滚：

```sh
sh /srv/auth-rust/infra/ops/deploy-production.sh rollback /srv/auth-rust/.local/production/deploy.json --allow-production
```

回滚拉取已记录前版digest，按原release.mjs核对真实兼容事实、重新备份、启动旧镜像和smoke，不运行down migration。不可逆迁移需独立窗口与恢复方案。首次部署的数据库备份仍需可用；脚本不会将“空数据库”变成跳过备份的理由。

正式域名、真实SMTP及DNS、独立备份/保留/RPO-RTO、设备/人工矩阵和上线24小时观察仍须按acceptance.md验收。此脚本已可审查并由CI产物携带，生产环境未准备时不会自动触发远程部署。
