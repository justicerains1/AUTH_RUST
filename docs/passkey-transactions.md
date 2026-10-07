# T09 Passkey 协议与存储

核心 [passkeys.rs](../crates/identity-core/src/passkeys.rs) 使用固定webauthn-rs0.5.5，origin/RP从已验证Config构造，RP_ID必须issuer host。HTTP仅本地允许；注册/断言由库检查challenge、origin、RP、签名和UV。认证结果再次检查user_verified=true；amr使用user，不声称硬件保护hwk。

注册options明确residentKey=required/requireResidentKey=true、UV=required，算法只ES256/RS256；库返回的凭证算法再次白名单核对，因此客户端更改options不能存入其他算法。服务要求clientExtensionResults.credProps.rk=true。rk是标准浏览器的未签名扩展，不能称为密码学resident或硬件attestation证明；最终无allowCredentials的discoverable签名登录由真实浏览器验证。库resident参数也不提供可签名的resident证明，本版保留这个明确标准边界，不使用修改过的库或虚假的额外证据。

注册/登录/重新认证状态只在服务器序列化，并AEAD绑定user与purpose，存五分钟单次数据库挑战；匿名discoverable登录使用nil UUID域加密状态，断言userHandle仅用于查已有credential/user，完成库签名验证后才可信。危险state-serialization feature用于跨进程权威存储，状态不发浏览器、不从客户端JSON读取。

存储 [store/passkeys.rs](../crates/identity-store/src/passkeys.rs) 先user锁再当前session/挑战/credential，检查verified/active/credential_version。首次因素绑定需近期password_confirmed_at，有因素新增/删除需近期strong_at。最多十个凭证，在同user锁内计数；credential ID唯一，不跨用户覆盖。注册成功保存库验证凭证、挑战消费、通知outbox与审计同事务。

discoverable options不需要邮箱，不泄账号；断言凭证ID/userHandle必须对应数据库既有用户。库authenticate后update_credential处理counter/backup事实，Some(false)允许无状态更新，None只凭证不匹配；不自行要求同步计数一概递增。凭证状态、last_used_at、消费挑战、随机session/CSRF、原授权事务绑定新session、预认证轮换与审计同提交。Passkey达到强认证，不追加TOTP。

重新认证绑定本人session/purpose，`reauth_only`在任何修改前核对，只更新strong_at不延长会话或改auth_time。错误断言累积失败预算，第五次消费；失败/成功审计不保存assertion、挑战明文或密钥。名称修改只本人认证，删除需强认证；当前账号始终有密码，不扩Passkey-only/管理员绕过MFA。

列表HMAC cursor绑定user/route/created_desc,id_desc、10分钟期限，limit1～100默认HTTP20，最多十凭证仍正确分页；GET/get/rename/delete本人归属。迁移 [0004](../migrations/0004_passkey_last_used.sql)记录真实last_used_at，首次null、每成功assert更新，rename不伪造最近使用时间。

Worker通知增加passkey.registered/renamed/removed，只说明安全变化，无credential秘密。核心options单测和编译通过；虚拟CDP真实签名注册/登录与负向协议由T09测试提供，真实手机/桌面设备与Safari仍需实测，不由虚拟认证器推定T09全部通过。
