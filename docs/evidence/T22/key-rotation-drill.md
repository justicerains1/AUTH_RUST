# T22 本地密钥维护演练

2026-10-08，显式执行 `cargo test -p identity-admin-cli --test t22_keys --locked`，提供KEY_DRILL_DATABASE_URL（仅identity_test）与受控KEY_DRILL_SIGNING_KEY_FILE。最终 **1通过/0失败/0ignored，退出0，0.41秒**；没有把缺环境跳过标通过。测试在独立生成的identity_test_key_drill schema中运行，完成后仅删除该schema，未停止开发PG/Redis或修改负载数据。

实际用旧/新AEAD key创建并重加密：TOTP种子、outbox、totp_enrollment、passkey_registration、passkey_reauthentication、nil UUID的passkey_login、BFF登录flow与session令牌。成功改写计数分别TOTP1/outbox1/challenge4/BFFflow1/BFFsession1；新key解密所有原文一致，TOTP真实六位代码仍匹配同counter，第二次改写为0。

篡改同user的passkey-registration加密tag后批处理失败，先处理的TOTP/outbox仍保留旧kid，证明同用户事务rollback；修复密文后重跑成功。Passkey/BFF测试payload是精确AAD下的合成opaque字节，只证明维护重包裹保留数据，不冒充真实Passkey硬件认证或BFF OAuth交换。

测试直接运行实际identity-keys二进制：public命令返回当前kid及仅公开JWK，没有d等私钥字段；reencrypt带版本化map/activekid/testschema命令在已完成数据上幂等成功，输出只计数不含payload。命令输入秘密文件/映射均临时或Git忽略，未保存到证据。

`cargo clippy -p identity-admin-cli --all-targets --locked -- -D warnings`最后退出0。默认单元测试只有纯rewrap规则；数据库演练位于显式integration target，缺环境返回错误。根fullsuite必须包含该目标，不能在发布报告跳过必要维护检查。

生产独立备份、真实签名key切换/分发与12小时旧公钥保留、生产TOTP/现有凭证恢复、正式RPO/RTO仍待实际运维环境验收；本地演练不等于T22整体放行。
