# 客户端秘密焦点修复后的最终edge制品

最终产品提交 `138815caa0978b8e6c906b99c53048932d8368cf` 已通过两浏览器12个真实可访问性用例，新增创建秘密关闭返回及Action→密码/恢复码强认证→Action→轮换结果关闭的键盘链路见[产品报告](product-accessibility/test-summary.md)。本次从该完整提交重新 `git archive` 至任务0700目录，按锁执行edge构建并验证。202项构建输入、54项制品摘要、36项实际运行stage退出0，见[最终结果](final-artifacts-20261009-focus.json)；旧[72dbe6a制品](final-artifacts-20261009.md)和失败历史保持。

| 实际本地镜像 | OCI image index | Image config |
| --- | --- | --- |
| `auth-rust-edge:t23-138815c` | `sha256:2e2cba1079c3050f7747d8019a2b7eccf5c556b4c5768c12b79b82e3a0205230` | `sha256:acd51ae8582a33101a3f138ec4690b73c10ef122c3ad30726d256c37d2438e73` |
| 复用 `auth-rust-runtime:t23-2c9c712` | `sha256:6080ddcd99ea92b53c750e11c206c7781a6cd4261d3c5cd515b8af75f4d07f9c` | `sha256:1d3bb5dee02c62c8a16de4a2fc8d8e53873ee82a705e0bf52ae46c7c5df2263b` |

新edge真实BuildKit linux/amd64 locked构建退出0，前端两生产文件变化触发实际三workspace build，相关固定依赖层复用缓存；不是无缓存构建。详细日志及摘要见[focus edge build](final-artifacts-20261009-focus-edge-build.txt)、[BuildKit元数据](final-artifacts-20261009-focus-edge-metadata.json)。镜像revision为138815c，Docker inspect ID与OCI/config各自记录。

Runtime实际release编译发生于2c9c712，没有重新编译为138815c。逐文件核对95项生产输入（Cargo/lock/toolchain/Dockerfile、crate生产源码、data/migrations）与138815c完全相同；`crates/identity-server/tests/t23_accessibility.rs`是唯一不同crate文件，它改变标准测试管理员/恢复码隔离，不参与`cargo build --workspace --release`六binary编译。Dockerfile COPY crates确包含测试文件，因此**整个Docker context并非完全相同**，报告明确列出此测试差异；复用既有release二进制不冒当前提交全量fresh runtime构建。

最终实际验证于2026-10-08T19:14:54Z～19:15:04Z（UTC；本地2026-10-09）完成0。复测同原权限约束：默认UID10001、read-only、cap-drop ALL、no-new-privileges、CapEff0/NoNewPrivs1，runtime网络none及六维护/运行命令配置、公钥只有public字段，world-readable指标秘密正确拒绝；Caddy上游能力为空并原配置实际启动，HTTP301精确Location。专属测试证书/CA注入只改变签发来源，原路由与安全头保持；三个站点真实hostname+CA TLS、HTML/JS完整SHA等于新镜像、HTML no-store/JS一年immutable、CSP/HSTS/nosniff/成功响应Server移除全部通过。

验证只使用独立随机秘密0600/RO挂载、专属容器和127.0.0.1随机TLS端口，全部已清理，不停止开发PG/Redis/SMTP。没有生产域名或正式ACME、SMTP、后台全链readiness、注册表发布或部署。无后端Caddy502仍显示低信息Server指纹的已知边界见前一[缓存制品报告](final-artifacts-20261009.md)，不把静态成功头测试扩张为所有错误页安全证明。参考设备、屏幕阅读器/完整zoom、独立存储/保留/通知/生产RPO-RTO与24小时高峰仍按原关卡待验收。
