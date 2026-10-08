# 签名与 AEAD 密钥维护操作稿

`identity-keys public`只输出Config当前/旧公钥JWKS，不生成或打印私钥。签名轮换先把新公钥加入公开JWKS，再切新私钥kid，旧公钥保留≥12小时+2分钟兼容退出hint；普通ID Token仍检查exp。私钥由受控秘密流程产生并owner-only挂载，不能自动替换生产文件。

`identity-keys reencrypt --keys-file <versioned-map> --active-kid <new-kid> [--batch-size 50]`载入含旧新key的owner-only文件，生产额外明确`--allow-production`，测试可用仅identity_test的`--test-schema`。数据库目标先验证，未知用途或任何解密/tag错误停止该事务并保留旧数据，不通过删除失败记录继续。

AAD映射精确：TOTP user/totp-seed，outbox user/email-outbox，待注册TOTP user/totp-enrollment，Passkey注册/重新认证 user及其原purpose，匿名Passkey登录nil UUID/passkey_login；BFF flow/session以记录UUID及`bff:namespace:flow|tokens`。库凭证公共验证数据不含AEAD私钥，无需猜测重加密。每批用户先锁user、因素/动作同事务更新；BFF专用行锁避免并发刷新覆盖。

旧key必须保留直到所有数据库记录和受控备份不再依赖，不能重加密后立即删除恢复所需旧key。维持单次nonce随机与kid版本，多进程重加密需受控调度；计数是改写行数，不含秘密。CLI基础编译/rewrap单元测试及 [本地数据库演练](../evidence/T22/key-rotation-drill.md) 已通过，覆盖全部当前加密用途、损坏tag rollback、真实TOTP代码与幂等执行。生产分发、签名切换和旧公钥时间窗口仍需实际验收，不推定生产轮换成功。

密钥回滚须与应用版本兼容：签名切回旧私钥kid时保留新旧公钥及两版私钥的独立受控备份，直到各自最后签发凭证的兼容窗口结束。AEAD切回旧active-kid也必须继续载入包含新旧key的版本map；已重加密的记录不能交给仅认识旧key的前版进程，否则会无法解密。每用户事务失败只回滚该用户；此前成功用户/独立BFF批次可能已提交，整轮维护不是一个全局事务。中止后保留所有key，核对已提交范围并幂等续跑，禁止删坏记录或误称整库已回滚。
