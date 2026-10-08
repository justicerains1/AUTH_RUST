# 2026-10-09 冻结源码生产镜像验证

最终前端/部署快照为 `72dbe6a4a85df0c72c672b5a8747860a17da5ecf`，Rust镜像编译快照为 `2c9c712c4fb83ebbb5d7e59fd97fcf9ef3b8777c`。两者均通过明确提交的 `git archive` 提取到0700任务目录，无工作区秘密或未提交源码进入构建；202项构建相关输入SHA、96项runtime输入逐项相同证明、54项制品SHA、36个实际检查及日志摘要见[最终结果](final-artifacts-20261009.json)。旧[1c0c9ea制品](final-artifacts.md)保留历史；生产Rust/前端/Compose后来已有实际修改，不能用旧摘要代表当前制品。

| 本地镜像 | OCI image index digest | Image config digest |
| --- | --- | --- |
| `auth-rust-runtime:t23-2c9c712` | `sha256:6080ddcd99ea92b53c750e11c206c7781a6cd4261d3c5cd515b8af75f4d07f9c` | `sha256:1d3bb5dee02c62c8a16de4a2fc8d8e53873ee82a705e0bf52ae46c7c5df2263b` |
| `auth-rust-edge:t23-72dbe6a` | `sha256:89adab27068eed9474fa9139ebfb29cfbcc500aed04353f0121f1089ffe2305e` | `sha256:5fcee57e8d05628d1df425874aed6ae848a44d9152f18c950bb0520f15945db5` |

这是本机linux/amd64制品，没有推送注册表或部署生产。BuildKit `--platform linux/amd64 --load`实测，源码label记录完整提交；运行时Docker inspect ID/RepoDigest、本地layer列表与OCI/config摘要分别记录。原runtime与edge构建在17:45:54～17:46:58 UTC退出0；Rust实际 `cargo build --workspace --release --locked`重编完成43.44秒，前端真实`npm ci`和三workspace `vite build`完成。Caddy修复后edge从72dbe6a再次归档构建，Rust及前端输入内容一致的层使用缓存，不声称无缓存重新编译。BuildKit结果见[runtime元数据](final-artifacts-20261009-runtime-metadata.json)、[edge元数据](final-artifacts-20261009-edge-metadata.json)，详细日志见[runtime构建](final-artifacts-20261009-runtime-build.txt)、[首次edge构建](final-artifacts-20261009-edge-build.txt)、[最终edge构建](final-artifacts-20261009-edge-final-build.txt)。

最终实际验证于17:57:51～17:58:02 UTC退出0，36项stage（包括准备/清理）成功，另包含一次正确非零的秘密权限负向检查：

- Runtime默认UID/GID10001、`--read-only --cap-drop ALL --security-opt no-new-privileges`与16MiB `/tmp` tmpfs、网络none；实际 `/proc/self/status` 为CapEff0/NoNewPrivs1，Docker inspect只读/能力/用户正确，`/app`写入失败。六命令可执行并取SHA，固定libssl3/CA/curl包版本匹配。
- 独立随机RSA/AEAD/SMTP/BFF/指标秘密0600、UID10001、只读挂载；实际执行server/worker/admin-cli `--check-config`，双BFF各配置校验，migrate `--help`，keys `public`。公钥只验证结构及无私钥字段，不进入证据；全部合法检查退出0。指标文件0644时server拒绝并非零，恢复0600；不连接数据库/Redis/SMTP，配置校验不等于后台真实服务健康。
- Edge默认UID10001、上述只读和能力约束；`getcap /usr/bin/caddy`为空、Caddy2.11.7可执行、`/srv`写入失败。原镜像Caddyfile实际validate并启动，仅`/data`/`/config`受控tmpfs可写，网络none；内部HTTP8080登录路径返回精确301及`https://identity.example.test/login`。
- 独立生成三合成example.test站点的测试证书，按原生产Caddyfile adapt保留全部路由/头部，仅注入该测试证书并禁用自动签发；专属网络/容器只发布127.0.0.1随机HTTPS端口。主机curl通过显式CA和hostname验证真实TLS，三个站点HTML与JS SHA逐项等于镜像制品；HTML no-store、JS public一年immutable、CSP/HSTS/nosniff、Server移除均通过。没有安装主机CA信任，没有正式域名或ACME签发，测试配置方法明确记录在JSON。

首次静态验证实际发现Caddy资产缓存缺陷：common_security同一header block中`-Server`使整组响应头被适配为deferred，父级`Cache-Control:no-store`最终覆盖资产的public immutable。最小修复将`header -Server`独立，不修改认证HTML/API no-store或安全头。候选修复实际三站HTTPS HTML/资产、HTTP301及无后端API/OAuth/BFF错误不可缓存检查12项通过，见[缓存修复结果](final-artifacts-20261009-cache-fix.json)；再从72dbe6a重建edge并通过上述完整制品验证。

真实失败证据保留：第一次验证断言未接受dpkg的`libssl3:amd64`架构后缀，实际版本正确，修测试定位；第二次捕获资产仍no-store并完成生产配置修复，见[资产失败](final-artifacts-20261009-failure-1791481839756.json)。候选检查曾额外要求无后端502也删除Server，实际Caddy错误响应仍暴露服务器指纹，见[候选失败](final-artifacts-20261009-cache-failure-1791481985799.json)；该低信息诊断属于已有错误页行为，本次没有扩大产品修改，正式安全扫描仍应评审。无后台502不当作readiness或认证成功；最终只对实际静态成功响应验证所有安全头。没有删除或掩盖原失败。

所有专属容器、网络和临时秘密已清理，没有停止共享开发服务。生产域名TLS/SMTP、实际全链业务冒烟、独立备份存储/WAL/保留、告警送达、参考性能硬件/真实高峰和24小时观察仍按原文档待验收；实体浏览器/Passkey/屏幕阅读器边界保持。这里证明提交对应的本地镜像与静态服务可运行，不能据此放行生产。
