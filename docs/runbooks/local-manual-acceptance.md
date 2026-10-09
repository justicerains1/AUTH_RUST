# Docker 本地人工验收

此部署使用 `infra/compose.dev.yaml` 与 `.local/dev.env`，环境为 development。按用户要求，所有发布端口绑定0.0.0.0（包含页面、API、BFF、PG、Redis与Mailpit），数据库、Redis、Mailpit数据使用持久卷。已重建当前工作区的后端与前端镜像，并启动bff profile；本地验收确认后再准备生产环境测试。

| 入口 | 地址 |
| --- | --- |
| 身份中心 | http://localhost:5173 |
| 管理后台 | http://localhost:5173/admin |
| 演示应用 A | http://localhost:5174 |
| 演示应用 B | http://localhost:5175 |
| 本地邮件箱 | http://localhost:8025 |

必须用 `localhost`，不要改成IP或其他域名；issuer、RP和应用回调固定使用这些地址。浏览器localhost属于可信本地上下文，可在支持的设备上测试Passkey。当前是本地HTTP开发Cookie配置，不代表生产HTTPS/Cookie验收。

管理员邮箱为 `admin@local.test`；随机密码只保存在本机受限文件 `.local/manual-acceptance/accounts.md`，不提交Git。本次自动验收已通过真实页面绑定管理员TOTP；登录需从该受限文件读取设置密钥导入自己的验证器，或使用尚未消费恢复码。后续自行保存恢复码，再进入管理后台；没有在数据库中伪造强认证。该文件还会记录一个已验证的普通验收用户。

建议依次检查：

1. 注册自己的测试邮箱，在Mailpit中打开验证链接，显式点击确认，再密码登录。
2. 绑定TOTP/Passkey，退出后验证登录、取消和密码回退；下载并安全保存恢复码。
3. 分别访问A/B登录，检查SSO；从身份中心退出所有设备，确认A/B下一次检查失效。
4. 两个独立浏览器登录同账号，撤销另一设备，检查其下一次请求拒绝。
5. 用管理员账号检查用户停启、客户端编辑/停启/秘密轮换、成员管理和最后管理员保护。
6. 检查手机宽度、键盘操作、错误提示、找回密码及Mailpit邮件。

你确认本地验收没有问题后，再准备生产域名、主机、SMTP和独立备份位置，进行生产部署与实际环境验收。本地部署不自动放行T09/T21/T22/T23。

运行状态与重启：

```sh
docker compose --env-file .local/dev.env -f infra/compose.dev.yaml --profile bff ps
docker compose --env-file .local/dev.env -f infra/compose.dev.yaml --profile bff up -d --wait
```

停止并保留数据：

```sh
docker compose --env-file .local/dev.env -f infra/compose.dev.yaml --profile bff stop
```

不要使用 `down -v`，不要覆盖 `.local` 密钥/密码。Windows使用WSL2时，可在Windows浏览器访问上述localhost入口；若Docker/WSL端口转发不可用，先核对Docker Desktop的WSL集成与localhost转发。

## 全网卡监听

Compose发布端口已改为`0.0.0.0`，即监听WSL宿主可用网络接口。Windows本机仍使用localhost入口；其他机器访问还取决于Windows防火墙、WSL网络模式和端口转发。应用issuer、RP与A/B回调仍固定localhost，直接用局域网IP登录不是同一配置，不能仅修改监听地址就保证跨机器认证有效。容器内健康检查仍访问127.0.0.1。

## Windows公网网卡到WSL转发

本机检查：Windows地址111.10.137.17，WSL地址172.24.66.254；Windows现只有localhost转发，公网IP访问5173失败，而Windows直接请求WSL地址5173返回200。用户选择只公开身份中心5173，其他端口不加入Windows转发/防火墙规则。

当前会话没有Windows管理员权限。以下在**管理员PowerShell**执行：

```powershell
& "\\wsl.localhost\Ubuntu-26.04.1\root\code\rust\auth_rust\infra\windows\forward-wsl-web.ps1" -Distribution "Ubuntu-26.04.1" -ListenAddress "111.10.137.17"
```

脚本只接受页面端口，默认5173；校验WSL目标可达，配置IP Helper、Windows指定网卡portproxy和对应TCP入站规则，然后检查Windows公网地址HTTP响应。可加`-Plan`只读预览、`-Remove`删除本脚本规则、`-AllowedRemoteAddress <允许的访问者IP>`限制来源。不会修改Windows全局防火墙开关，不开放数据库、Redis或SMTP。脚本语法与只读预览已实际通过；管理员写入步骤仍待用户执行，不能说已转发成功。

WSL重启后地址可能变化，重新执行刷新目标。若Windows本机公网地址已可访问、外网仍失败，继续核对上游路由/运营商/安全设备的5173入站限制。仅打通端口不改变issuer/RP/邮件和SSO地址：当前认证仍是localhost，非loopback公网完整认证需HTTPS域名和对应配置；Vite也可能拒绝未允许Host，需要按实际错误配置，不能直接开放全部Host。
