# T09 WebAuthn 依赖来源

2026-10-08实际访问crates.io `/api/v1/crates/webauthn-rs`：稳定0.5.5、notyanked、MSRV1.88、MPL-2.0。源码Cargo.toml声明与registry一致，manifest SHA256 `f8b01b4e390d4d955f370a92bd96fa3549117e4fdf0358c4f550d0f01167fb51`。GitHub https://github.com/kanidm/webauthn-rs 实际archived=false/disabled=false、pushed_at=2026-10-03T02:22:32Z。

固定webauthn-rs0.5.5及锁文件，conditional-ui用于discoverable，danger-allow-state-serialisation仅服务器AEAD加密数据库状态。未修改第三方库，MPL许可按依赖声明保留；不误记MIT。传递webauthn-rs-core/proto/attestation-ca同0.5.5，原库OpenSSL验证编译已真实成功，依赖公告最终由根RustSec报告确认。

实际读取源码确认：start_passkey_registration强制UV但默认residentkeyfalse；本平台options设required并要求标准rk扩展true，记录未签名扩展的保证边界。register_credential不验证resident字段为密码学属性，不能虚构硬件证明。start_discoverable_authentication强制UV，finish结果user_verified再次检查；Passkey.update_credential只在不匹配时None，Somefalse不能因计数未变化拒绝。

`cargo test -p identity-core --lib passkeys::tests --locked`真实1项通过，核固定RP/origin、required UV/resident、无账号allowCredentials、无效识别拒绝；`cargo clippy -p identity-core -p identity-store -p identity-worker --lib --bins --locked -- -D warnings`真实退出0。仅这些检查不能替代真实WebAuthn签名/浏览器负向/恢复设备验收，后续原始证据与真实设备阻塞在T09记录。
