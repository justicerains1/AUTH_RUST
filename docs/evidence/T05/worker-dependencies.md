# T05 Worker依赖与本模块验证

- 日期：2026-10-07 UTC。
- 关联：T05.05～T05.08。
- 新依赖：lettre精确0.11.23，MIT，MSRV1.85，crates.io当前max_stable_version、notyanked。
- 实际来源：https://crates.io/api/v1/crates/lettre 与 https://github.com/lettre/lettre。
- 下载源码Cargo.toml许可/repository与registry metadata一致；源码manifest SHA256 `3a21e86035209b19b4e4016243274b7d99cfdaa8296a87859f4aaad52781a281`。
- GitHub实际archived=false/disabled=false，pushed_at=2026-10-02T18:22:12Z。

使用builder/smtp-transport/tokio1-rustls-tls/pool/hostname；生产required STARTTLS源码明确Tls::Required，不调用危险证书关闭选项；开发明文仅Config允许本机/Mailpit。

本模块真实执行：`cargo check -p identity-worker`退出0并更新锁；`cargo test -p identity-worker --lib --locked`2项通过（重试1/5/15/60/180及终止边界、固定issuer/路径/fragment/HTML转义）；`cargo clippy -p identity-worker --all-targets --locked -- -D warnings`退出0。首次Clippy冗余闭包失败已修，不标为初次通过。

首次在线RustSec更新因Git IO失败1；随后`cargo audit --no-fetch --json`对当前锁实际退出0。此结果只基于本机当前公告库，不声称在线更新成功；最终root汇总的完整安全报告与commit为准。

Worker真实SMTP投递、Mailpit停止恢复、重复租约及API端到端由T05集成执行器运行并填写证据，本文件不以模块编译替代T05-MAIL-01～04通过。实现与配置说明见 [email.md](../../email.md)。
