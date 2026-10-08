# T14 管理员 CLI 依赖与模块证据

2026-10-08实际访问crates.io `/api/v1/crates/rpassword`：当前稳定7.5.4、notyanked、Apache-2.0、MSRV1.85，仓库 https://github.com/conradkleinespel/rpassword 。下载源码Cargo.toml与metadata一致，manifest SHA256 `6d6f8e97014d723398630d43f4da1d7b1b1cf8d10ba81b7da82b2a95bb5738ac`。精确依赖已锁，隐藏输入不自行操作危险终端API。

真实执行：

- `cargo check -p identity-admin-cli --offline`退出0，更新CLI rpassword/sqlx依赖metadata。
- `cargo check --workspace --locked`退出0，全部模块可用同锁文件。
- `cargo clippy -p identity-store -p identity-admin-cli --all-targets --locked -- -D warnings`退出0（没有禁用规则）。
- `cargo test -p identity-admin-cli --bin identity-admin-cli --locked`退出0，1项CLI边界测试通过：128个4字节Unicode加CRLF允许、超长/多行/非法UTF8拒、密码空格保留。
- `cargo audit --no-fetch --json`实际退出0，366依赖、0漏洞、warnings空，缓存1293公告；工具输出last-commit/last-updated为空，因此不宣称在线更新或编造公告日期。根最终在线/缓存摘要由其独立证据记录。

管理员初始化采用私有验证能力值、全局事务锁及用户版本重验；所有后台危险变更与真实审计同事务，必要拒绝保留denied审计。用户名、密码、secret、token从不输出。详细事务和适用范围见 [admin-transactions.md](../../admin-transactions.md)。

本记录不以单测/编译冒充CLI真实并发、HTTP后台权限/恢复或T20完整审计；T14真实4案例与最后因素保护测试由集成执行器记录。
