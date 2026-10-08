# E08 真实 MFA 账号密码重置组合验收

2026-10-08 UTC，测试基于 `fbe610d` 后未提交测试补充，最终源码提交以 Git 历史追踪。新增 `t07_real_mfa_reset` 并接入 T07 集成的必跑分支，没有修改生产源码或删除历史因素保留边界测试。

同一账号通过真实 API 登记并确认 TOTP，拥有两条有效会话，再实际授权、同意及兑换授权码得到访问/刷新令牌。通过 Worker 和 Mailpit 取得真实重置邮件，确认更改密码后，两条旧会话均 401、旧 access/refresh introspection inactive、旧 refresh 兑换 `invalid_grant`、活动授权数为零，且没有自动登录。新密码仍产生受限 MFA 挑战，原始 TOTP 在可控时钟的新时间步完成真实登录；因素 ID、密文、kid 和 nonce 保持。

`npm run test:integration -- --task=T07` 于 2026-10-08T13:34:40Z 完成、退出 0，原 T07 套件和新增组合测试都实际执行。定向 Clippy、runner ESLint、diff 检查退出 0。公开 [T07 输出](../T07/integration.txt) 仅包含案例标识；原始进程诊断保存在 `.local/t07-e08-real-reset-command.txt`。

这个新增案例使用真实 TOTP，没有用不可用的合成因素证明登录，也没有清除重放状态。Passkey 实体设备仍需 T09 独立验收；完整回归还须包含这次新增测试，生产 SMTP/设备条件不会由 Mailpit 结果释放。
