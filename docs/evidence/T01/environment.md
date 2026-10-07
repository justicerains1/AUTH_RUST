# T01 实际环境

- 日期：2026-10-07T10:37:45.695037+00:00
- 目录：`/root/code/rust/auth_rust`；无 Git 仓库，代码版本以 source-sha256.txt 标识，不编造 commit。
- OS：Linux-6.18.40.1-microsoft-standard-WSL2-x86_64-with-glibc2.43
- CPU：16 logical CPUs；MemTotal:       15956560 kB
- Rust 1.98.0，Node 22.22.1，宿主 npm 9.2.0，容器 npm 10.9.4。
- Docker Engine 29.1.3，Compose 2.40.3，buildx 0.30.1；PostgreSQL 17.11、Redis 7.4.11、Mailpit 1.31.4。
- APP_ENV=development；数据库 identity_development（无业务表）。production 仅配置拒绝测试，无生产部署。
- 本机 Docker 真实服务；不代替 Windows/Safari/真实认证器或生产验收。
