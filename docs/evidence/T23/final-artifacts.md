# T23 最终源码生产镜像验证

2026-10-08 UTC，Linux/WSL2 Docker/BuildKit，Linuxamd64。最终验证快照为`1c0c9eaace5c8ddb9fd77803d6bc42b5abf967f8`（已含`946dbb0`运维修复及Caddy能力修复）。两个build context均通过`git archive`从明确提交提取到任务私有目录，没有使用工作区未提交文件或秘密。210个相关源文件SHA256、54个制品摘要、12项运行检查及失败记录见[final-artifacts.json](final-artifacts.json)。

| 本地制品 | 最终OCI image index摘要 | 构建 |
|---|---|---|
| `auth-rust-runtime:t23-1c0c9ea` | `sha256:ea2cdeb4d76ab1db6b06145e14b43aacd17b47467eb261a931c52665950fa0f5` | 退出0 |
| `auth-rust-edge:t23-1c0c9ea` | `sha256:e7a82f181cd3f483e9b98b099a1e05126732a483f1bc865dbbf5b5ee9a91ad6f` | 退出0 |

这些是本地构建产物，没有向registry发布，也没有生产部署。Rust和三前端源码在`319f30c`至最终提交之间相同；前一次真实`cargo build --workspace --release --locked`完成56.32秒，三前端`npm run build --workspaces`实际完成。最终两个提交对齐的构建复用了这些源内容匹配的BuildKit层，不声称无缓存重编。前端安装按lock执行`npm ci`；没有降低依赖/安全参数。

完整日志分别为[首次Rust构建](final-artifacts-initial-runtime-build.txt)、[首次前端构建](final-artifacts-initial-edge-build.txt)、[Caddy修复构建](final-artifacts-edge-capfix-build.txt)、[最终Rust构建](final-artifacts-final-runtime-build.txt)和[最终edge构建](final-artifacts-final-edge-build.txt)，文件SHA256见JSON。最终image config摘要与OCI index摘要分别记录，避免混淆。

## 与生产限制一致的运行验证

所有正式运行检查均采用镜像默认UID/GID10001、`--network none --read-only --cap-drop ALL --security-opt no-new-privileges`，只为`/tmp`提供16MiB tmpfs。没有公开主机端口，没有启动或停止共享开发PG/Redis。

- Runtime实际检查六个命令可执行：identity-server、identity-worker、identity-admin-cli、identity-migrate、demo-bff、identity-keys；`/app`写入失败。OpenSSL3.0.22、libssl3 `3.0.22-1~deb12u1`、CA `20250419~deb12u1`、curl `7.88.1-10+deb12u15`与固定包一致。
- 用独立生成的临时RSA/AEAD/SMTP/BFF/指标秘密（0600、UID10001、RO挂载）实际运行API/Worker/admin/BFF `--check-config`，以及migrate `--help`，全部退出0；network-none且仅check-config，不连接外部服务。秘密随后删除。
- 实际`identity-keys public`退出0，输出1个对应合成kid的公钥，检查无`d/p/q/dp/dq/qi/oth`私钥字段；未把公钥payload或秘密复制入证据。该命令没有实现`--help`，缺命令实际返回usage/退出1并被正确当作预期失败。
- 指标秘密改成0644时实际API配置校验拒绝，退出1且仅写字段名；恢复0600后配置成功。此负向结果是预期拒绝，不计作成功启动。
- Edge实际`getcap /usr/bin/caddy`为空，Caddy2.11.7，`/srv`写入失败。合成域名下`caddy validate`退出0；真实域名/证书没有测试。
- 启动任务独有edge容器，只有`/data`、`/config`提供可写受控目录，保持上述生产限制；实际内部HTTP8080 `/login`返回**301**及精确`Location: https://identity.example/login`。没有主机端口；network-none阻断ACME/DNS，所以不宣称证书签发或HTTPS业务冒烟。该容器及状态目录已删除。

## 实际缺陷与失败复测

首次`319f30c` edge构建成功，但生产`cap_drop ALL`约束下Caddy执行**255失败**：`exec /usr/bin/caddy: operation not permitted`。只读检查确认上游二进制携带`cap_net_bind_service=ep`，与移除全部能力冲突；仅read-only或no-new-privileges检查无法发现该条件。使用高端口8080/8443无需此能力，Dockerfile追加`setcap -r /usr/bin/caddy`；保留生产限制，修后实际validate/version/启动均通过，再从提交`1c0c9ea`重建与验证。

临时验证脚本另出现两次真实非零：负向配置断言最初只读stderr，实际结构日志在stdout；首次跳转断言误写308，实际Caddy `permanent`返回301。修正验证脚本后按真实合约重跑；产品源码未因此放宽，失败原因和实际状态写入JSON。

这次检查证明最终提交的本地镜像可以按规定限制运行并包含维护工具；T22/T23生产关卡仍待真实域名TLS、SMTP/DNS、独立备份和完整恢复、监控送达、部署回滚与24小时/实际高峰观察。单机维护中断与实体设备条件继续保留，不能从镜像检查推定发布成功。
