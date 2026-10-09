# Docker 本地人工验收

此部署使用 `infra/compose.dev.yaml` 与 `.local/dev.env`，环境为 development。所有入口绑定127.0.0.1，数据库、Redis、Mailpit数据使用持久卷。已重建当前工作区的后端与前端镜像，并启动bff profile；本地验收确认后再准备生产环境测试。

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
