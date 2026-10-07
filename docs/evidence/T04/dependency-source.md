# T04 core 安全依赖、弱密码来源与实际验证

日期：2026-10-07 UTC。关联 T04.01/T04.02/T04.03/T04.08。本阶段实现本地安全原语，不宣称注册、SMTP或OAuth业务已实现。

## 直接依赖核查

实际访问 crates.io `/api/v1/crates/{name}` 并读取下载源码 Cargo.toml。以下精确版本均为当前 max_stable_version、not yanked；源码license/repository与registry metadata一致。Rust1.98满足声明MSRV，subtle未声明MSRV，实际编译和测试成功。

| crate | 精确版本 | 源码许可证 | MSRV | 上游 |
|---|---|---|---|---|
| argon2 | 0.6.0 | MIT OR Apache-2.0 | 1.85 | https://github.com/RustCrypto/password-hashes |
| aes-gcm | 0.11.1 | Apache-2.0 OR MIT | 1.85 | https://github.com/RustCrypto/AEADs |
| getrandom | 0.4.3 | MIT OR Apache-2.0 | 1.85 | https://github.com/rust-random/getrandom |
| sha2 | 0.11.0 | MIT OR Apache-2.0 | 1.85 | https://github.com/RustCrypto/hashes |
| hmac | 0.13.0 | MIT OR Apache-2.0 | 1.85 | https://github.com/RustCrypto/MACs |
| subtle | 2.6.1 | BSD-3-Clause | 未声明 | https://github.com/dalek-cryptography/subtle |

Argon2和AES-GCM启用zeroize，CSPRNG直接getrandom::fill；password-hash/phc通过argon2既有依赖接口，不新增不需要的rand/RNG路线。tokio启用sync用于进程共享预算。实际GitHub仓库API核查维护状态如下，均未archived/disabled；这不是无漏洞保证：

| 仓库 | 最近push UTC |
|---|---|
| https://github.com/RustCrypto/AEADs | 2026-09-14T14:05:14Z |
| https://github.com/RustCrypto/MACs | 2026-09-21T17:52:52Z |
| https://github.com/RustCrypto/hashes | 2026-10-06T13:13:47Z |
| https://github.com/RustCrypto/password-hashes | 2026-09-21T05:53:13Z |
| https://github.com/dalek-cryptography/subtle | 2024-08-03T23:10:52Z |
| https://github.com/rust-random/getrandom | 2026-09-21T16:04:28Z |

## 弱密码清单

清单实际请求SecLists的固定commit `913b327317496d062bcc7cace524aaad8a693be2`：

- [原始清单](https://raw.githubusercontent.com/danielmiessler/SecLists/913b327317496d062bcc7cace524aaad8a693be2/Passwords/Common-Credentials/10k-most-common.txt)。
- [该commit的MIT许可证](https://raw.githubusercontent.com/danielmiessler/SecLists/913b327317496d062bcc7cace524aaad8a693be2/LICENSE)。
- [本地清单](../../../data/weak-passwords.txt)、[完整许可](../../../data/weak-passwords-LICENSE.txt)、[版本与原始摘要metadata](../../../data/weak-passwords.metadata.json)。

两固定URL实际成功读取。清单原始73026字节、10001行，原始SHA-256 `68782d6a4a19a4768d5f15dd66bd534e7a33055cc755411e33f16d18c50fdcce`；SHA-512 `15c498d560d2bb06b6c04a5ac80a51f68e75950fe979f72dcd38273f50dcf256dfecbdca309d425799bd7713beaa40d36c93a902da3f7f560f9469cd1e789e18`。许可1072字节、SHA-256 `3dbdc93d5f8829de0941744841730a09c106d0732e5ae0e98ca1d77be7ded66c`。没有转换源码字节，MIT版权及完整许可随清单保留。

Password::new按本地精确匹配，不trim/大小写归一化密码，不把用户密码发往任何外部服务。这是有限常用密码基线，不等于全部泄露密码数据库；绝大多数短清单项同时被15字符最小长度拒绝。版本升级必须更新metadata和SHA校验测试，生产后续可扩充同许可的版本化本地语料，但不能声称现清单覆盖所有弱密码。

## 密码与crypto接口约束

normalize_email只接受ASCII，拒控制字符，trim后小写；local最多64/全址254，拒空标签/非法域字符/标签>63、local点号非法位置，保留点/+tag。Password::new允许空格与Unicode，15～128字符和512UTF8字节；登录verify允许旧短密码但上限不放宽。

PasswordService::initialize全进程共享OnceLock semaphore和OnceCell dummy hash，同配置调用/克隆不增加预算；首个配置1～4并行任务，最大等待250ms，spawn_blocking持有permit直至任务结束。dummy随机密码预算一次，同Argon2id m=65536KiB/t=3/p=1/output32；未知账号只verify该dummy。PHC验证真实读取编码参数；旧较弱hash可needs_upgrade，较高p值不因不同p而自动降级。PHC损坏/超资源包络拒绝，当前最大接受m=262144/t=10/p=4，超此范围需显式策略升级。

Token32字节、RecoveryCode16字节来自OS CSPRNG，不打印原文；DB使用sha256摘要。常量时间比较用subtle。HMAC-SHA256使用固定32字节key，padding到SHA256 block64与标准HMAC语义一致，并通过RFC4231 vector实测。purpose/value显式长度前缀防拼接碰撞。

AeadKeyRing严格拒重复kid（复用config唯一map解析器）、空/错误key/全零key；随机12字节nonce，每次AES256GCM将固定协议域、UUID用户、purpose长度/内容、kid长度/内容作为AAD。保留旧版本解密、当前版本加密，派生HMAC key用独立用途域。plaintext/password/hash/key持有Zeroizing，Debug过滤秘密，envelope只含kid/nonce/ciphertext。load_file启动调用前仍由Config负责生产文件权限。

## 真实安全测试

`cargo test -p identity-core --lib security::tests --locked` 退出0，8项通过，耗时11.67秒：

- 邮箱ASCII/别名/控制字符/非法域/64本地与254总长边界。
- 密码Unicode15/128、4字节UTF8最大512、空间保留、拒超长、Debug脱敏。
- 清单原始SHA256和10001条，实际允许长度的清单项仍拒绝。
- 128个随机256位token唯一，恢复码128位，固定长度常量比较及Debug脱敏。
- HMAC RFC4231 test1输出与用途长度碰撞分离。
- AEAD原文往返、错误用户/用途/nonce/ciphertext/key版本、独立nonce、旧key兼容、全零key拒绝。
- 加密key文件重复kid拒绝。
- 真实64MiB Argon2id正确/错误密码、真实8MiB旧PHC参数验证和upgrade、dummy指针/摘要不重建、共享预算及250ms超时后恢复、低基数统计非零。

`cargo clippy -p identity-core --all-targets --locked -- -D warnings` 退出0。第一次core编译因当前HMAC keySize API类型错误真实失败，改为标准block64 padding并用已知vector复测，不删除失败。

## RustSec

新锁第一次在线 `cargo audit --json` 因RustSec Git IO/network失败1；旧缓存no-fetch对新锁扫描退出0。随后主代理在线重试实际成功0，最终证据为 [cargo-audit.json](cargo-audit.json)：270依赖、0漏洞、warnings为空，1293公告、commit `b0797f54ea5d1d5bc1266bff06e201d1c5e07dca`，last-updated `2026-10-07T14:00:26+02:00`，JSON SHA256 `65614761824458641fbcf3ddf954141b46441f4689aac065256e163dcb58e294`。

只证明当次锁文件/公告库扫描，T20完整协议、安全、ZAP与生产扫描仍需单独执行。T04整体案例由根测试和真实PG/Redis中间件验收汇总；本core测试不能替代这些行为。
