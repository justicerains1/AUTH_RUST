# T09 真实自动化验证边界

2026-10-07 UTC（上海时间为 2026-10-08）。PostgreSQL 使用独立 identity_test 临时 schema；Redis 使用随机测试密钥命名空间；浏览器 Chromium CDP 虚拟 CTAP2 认证器启用 resident credential、用户验证。

真实 API 策略测试通过：注册 options 的近期认证、required resident/UV、加密服务端一次性状态、跨账号删除拒绝及最多十凭证。计数/归属用的预置凭证是明确的数据库策略 fixture，不作为 WebAuthn 认证成功证据。

Playwright 两个案例通过：真实虚拟认证器注册、退出后 discoverable 签名登录、Passkey 强重新认证后修改密码；对尚未消费的真实签名证明，分别修改 origin、challenge、signature 后服务端返回 401/403，同一原始合法证明成功 200，再重放原证明被拒。

无 UV 验证使用新鲜挑战：从 CDP 的测试虚拟认证器取出私钥，仅保留在进程内，将真实 authenticatorData 的 UV 位清零，再用该真实私钥重新生成有效 ECDSA 签名。服务端 required UV 策略拒绝 401/403，而同挑战原始带 UV 的合法证明随后成功 200。这验证的是实际签名与库策略，不是前端取消提示或已消费状态遮蔽的失败。没有修改生产策略，也没有将私钥、credential、assertion、Cookie 或授权秘密写入报告。

截图、video、trace 均关闭；错误诊断中的长 base64url 数据和测试密码脱敏。早期标准 JSON 属性大小写与 options 扩展契约失败记录保留；修复后最终 E2E 明确 2 passed / 0 failed。

T09-PK-04 真实手机、桌面实体认证器及 Safari 仍未执行，本环境只有虚拟 Chromium 认证器。该案例应保持阻塞或待验收，不用自动化结果替代。
