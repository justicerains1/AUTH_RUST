# T02 文档校正

- plan.md 6.2 Passkey 原表仅 GET/{id}，无法满足同文T09/T18列表需求：明确 GET /me/passkeys 列表及 GET/PATCH/DELETE /me/passkeys/{id}。仅补齐已有管理范围，不新增认证方式。
- OIDC Core 3.1.2.1要求 authorize 支持GET和POST，原6.1只GET：补POST form并共用参数校验，仅创建短期流程。同步T10-AUTHZ-01覆盖两method；GET不提交同意的安全边界保留。
- RP-Initiated Logout要求退出入口支持GET/POST：补 `/oauth/logout` POST form，入口仅校验并确认；`/oauth/logout/confirm` 才验证Origin/CSRF后撤销，同步T12-REV-04。
- 原7.2验证邮箱锁动作与用户，与5.3全局用户→会话→grant→action矛盾：明确先用户后action，避免锁序不一致。
- 后续页面/跨模块验收隐性依赖已审计；进入T05前同步具体阶段验收方案，不能用mock或零测试消解。

- 原邮箱/密码恢复/TOTP表省略的POST /confirm路径补全为各命名空间，消除实现路径歧义，保持原流程。

- RFC7009/7662 token_type_hint未知值是提示，必须忽略；OpenAPI允许有界ASCII hint并保持实际token验证，unsupported_token_type仅用于不支持的真实令牌类型。
