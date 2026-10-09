# Nginx检测、Docker Caddy与接入指南

安装器自动检测nginx -v与systemctl active。未运行使用CI edge成品内Caddy，不另安装宿主代理；运行时明确询问是否停止，默认保留并退出。切换前实际尝试绑定80/443检测冲突，冲突尝试恢复Nginx；成功切换后询问是否禁用Nginx开机启动，原配置不删除。域名仍由安装用户逐项输入，Caddy通过环境设置运行域名/自动证书。

[19测试](tests.txt)全部0：新增缺Nginx选择、拒切换不stop、冲突恢复以及真实occupied socket检测，原真实PG备份/加密解密、Cookie/CSRF、CI打包和迁移顺序保持。nginx systemctl动作使用测试executor验证，没有实际停止本机/生产Nginx，不声称新主机完整切换已通过。ESLint/bash语法/文档OpenAPI/diff0。

新增面向开发者的[接入指南](../../../developer-integration.md)，按真实discovery/已有BFF说明客户端申请、配置、登录退出代码、state/nonce/PKCE、当前身份/刷新撤销、排错和验收。修正原BFF开发迁移命令缺明确env。CI部署包携带指南/BFF存储说明，所有URL和账号参数是需替换示例，没有写入用户秘密。
