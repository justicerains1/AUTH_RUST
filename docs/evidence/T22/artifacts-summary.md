# T22 生产制品子模块验证

当前仅交付Dockerfile/Compose/Caddy与单机操作稿；T20/T21依赖、真实生产/恢复/SMTP条件未完成，T22整体不标通过。

下面本地review镜像digest对应当次构建时源码快照，构建早于后续metrics/密钥维护命令等变更；它们不是最终最新发布镜像。正式发布必须从最终提交重新locked构建、记录新checksum/digest并更新受控部署引用。当前仅Git提交，不声称已向Docker registry发布。

实际核查并pull成功的新增镜像：

- Debian `bookworm-20261005-slim`（官方12.15），manifest index SHA256 `7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587`。
- Caddy `2.11.7-alpine`（官方release2026-10-03），index SHA256 `d8542f48d34a9cf4e4c11a478865229840e87e4c96ea3f439101f31a5d35f75f`。

实际来源docker.m.daocloud.io V2 manifest API，pull两镜像退出0；DockerHub网络回退沿T01来源记录，不声明签名认证。apt-cache实际查询后固定libssl3=3.0.22-1~deb12u1、ca-certificates=20250419~deb12u1、curl=7.88.1-10+deb12u15，runtime版本检查一致。

2026-10-08真实执行：

- `docker build -f infra/Dockerfile.edge.prod -t auth-rust-edge:t22-review .`退出0：npm ci和三前端productionbuild真实执行。补明确HTTP8080→外部HTTPS443跳转规则后缓存重建也退出0，最终index SHA256 `8522387abef196af6c01b02102baac30be0c71b453716d4ec231fb9fd6ff0785`。
- `docker build -f infra/Dockerfile.prod -t auth-rust-runtime:t22-review .`退出0：locked workspace Rustrelease，结果index SHA256 `f223db024b80d8bcdd17fa8a231269b8b025062c9a5dbd06b65a2ef9e9d2775b`。
- runtime无网络/read_only/tmpfs运行检查UID10001、5binary可执行、OpenSSL3.0.22/CA/curl版本及/app不能写，退出0。
- edge无网络/read_only执行Caddy validate，三个合成example域和无真实证书配置，退出0。
- Compose验证副本只把未提供production.env替成已有本地env用于`config --quiet`，退出0；未运行生产Compose或向外部域名发部署操作。

这些只证明制品构建和配置有效，不证明TLS真实申请、SMTP生产投递、备份/WAL恢复、轮换或告警可达。真实发布应使用版本镜像digest、受控秘密权限和独立备份，按 [single-host-deployment.md](../../runbooks/single-host-deployment.md) 顺序执行。


最新制品已从提交1c0c9ea重新归档构建并按完整权限约束实测，runtime六命令及Caddy启动通过，见[T23最终镜像](../T23/final-artifacts.md)。上述早期摘要继续只代表历史快照；生产部署和注册表发布仍未执行。
