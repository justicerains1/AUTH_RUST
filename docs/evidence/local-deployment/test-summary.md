# Docker本地人工验收部署

当前工作区已重新构建Docker后端和三前端，启动bff profile，10/10服务健康；显式development数据库迁移与A/B客户端校验通过。保留原数据库/缓存/邮箱卷及受控密钥。

真实浏览器注册→Mailpit邮件→显式确认→密码登录→A/B分别授权SSO→退出所有设备→两BFF下次401→首管理员登录均通过。首次冒烟用错退出按钮名称，第二次在退出请求完成前断言BFF，测试改为真实logout-all204响应同步后通过；未修改生产代码或认证规则。管理员未预绑定因素，人工首次登录按页面绑定TOTP/Passkey。

[部署记录](deployment.json)、[访问与验收操作](../../runbooks/local-manual-acceptance.md)已保存。密码仅受限.local文件，不入Git；服务继续运行供用户验收。当前development/localHTTP部署不是生产HTTPS/SMTP或正式发布验收。
