# T10 授权与同意真实验收

2026-10-08。受管理client随机secret仅摘要、完整回调精确匹配；Authorization Code+PKCE S256的GET/POSTform共严格参数解析，状态/nonce/PKCE/prompt/maxage/scopes固定在5分钟浏览器事务。登录后事务从预认证绑定迁移至新会话。

真实PG/Redis/API四案例通过，扩充实际promptlogin要求新密码session、maxage旧认证拒绝、scope扩展需同意、promptnone标准login/consenterrorstate、跨browser事务不可读/决定、十并发decision最多一code/grant、客户端停用不签授权码。批准生成60秒单次code，拒绝access_denied，只向已注册回调返回。精确参数白名单拒未知return_to，不接受 arbitraryredirect。

真实Playwright2流程同源authorize→登录→同意/拒绝→回调通过，36前端测试、62工具、Rust单元与Clippy/TS/ESLint/fmt、build通过，0005开发迁移0。T09实体设备待验收不阻T10自身前置，但发布仍未放行。

先前PKCE示例C重复43非canonicalbase64url被正确拒，修示例不改decoder；开发代理仅/api导致前后端不同origin，新增精确OAuth协议代理保留前端同意route；未知参数静默忽略已严格白名单拒绝；code总量测试改为场景前后一次增量。失败脱敏报告保留。T11令牌交换/签名和T12刷新/撤销未在此任务假称实现，后续继续。
