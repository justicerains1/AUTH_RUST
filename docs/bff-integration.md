# T13 服务端 BFF 接入约定

同一 Rust `demo-bff` 包以两份独立配置运行应用 A/B。前端只接收本应用随机 HttpOnly Cookie、用户 UUID 和 CSRF；OAuth access/refresh/ID token 与 client secret 留在服务端。数据库长期令牌使用版本化 AEAD，并绑定 namespace/session UUID，详见 [持久化与共享锁](bff-storage.md)。

开发者首次接入可先阅读[接入指南](developer-integration.md)，按客户端申请、配置、浏览器流程及验收步骤实施。

## 配置与开发接入

| 字段 | A 示例 | B 示例 |
|---|---|---|
| APP_ENV | development | development |
| BIND | 0.0.0.0:8082 | 0.0.0.0:8083 |
| BFF_PUBLIC_ORIGIN | http://localhost:5174 | http://localhost:5175 |
| ISSUER | http://localhost:5173 | http://localhost:5173 |
| BFF_CLIENT_ID | demo-a | demo-b |
| BFF_NAMESPACE | demo_a | demo_b |
| BFF_COOKIE_NAME | demo-a-session-dev | demo-b-session-dev |
| BFF_CLIENT_SECRET_FILE | /run/secrets/demo-a-secret | /run/secrets/demo-b-secret |
| BFF_ISSUER_CONNECT_HOST | identity-web（仅开发容器） | identity-web（仅开发容器） |

`DATABASE_URL`、`ENCRYPTION_KEYS_FILE`、`ACTIVE_ENCRYPTION_KID` 使用受控服务配置。数据库 URL 必须是带用户名、主机和数据库路径的 PostgreSQL URL；错误仅显示配置字段，不打印 URL/秘密。密钥文件在启动时验证格式和 active kid。生产 public origin/issuer 必须 HTTPS，Cookie 使用各自 `__Host-` 前缀、Secure/HttpOnly/Path=/、SameSite=Lax 且无 Domain；Unix 上秘密及 AEAD 文件必须禁止组/其他用户读取。秘密文件最多 1 KiB、AEAD 文件最多 1 MiB。生产禁止 DNS 连接覆盖。

从仓库根目录运行：

```sh
npm run dev:secrets
npm run dev:up
npm run db:migrate -- --env=development
npm run dev:clients
docker compose --env-file .local/dev.env -f infra/compose.dev.yaml --profile bff up -d --wait --build
```

访问 A `http://localhost:5174`、B `http://localhost:5175`；身份平台为 `http://localhost:5173`。`dev:up` 默认基础服务保持不变，双 BFF 由 `bff` profile 启用。`npm run dev:clients` 只初始化受控本地 development 数据库；Rust CLI 可接收明确的 development/test 目标，不允许 production。它注册各自完整 `/bff/callback`、openid/profile/email scopes，随机秘密仅写独立本地 0600 文件；已有客户端只校验并复用匹配秘密，不打印秘密或覆盖冲突配置。秘密不提交到仓库。

开发 Vite 代理 `/bff` 到各自 BFF；身份 Vite 精确代理 `/.well-known/openid-configuration` 和实际协议/API 路由。同源 Cookie 保留 localhost origin。容器 SDK 仅接受 `BFF_ISSUER_CONNECT_HOST=identity-web`，将固定 issuer 域名的连接地址解析为可信 Compose 服务，同时保留原 Host/URL/声明。所有 metadata endpoint 仍须属于固定 issuer origin，HTTP client 不跟随重定向。

生产部署前分别创建两个机密客户端和完整回调，使用各自秘密、namespace、Cookie 与 origin；反向代理将同源 `/bff` 转发给对应实例。多个同应用 BFF 实例必须连接同一个权威 PostgreSQL，并使用兼容的 AEAD 密钥版本。运行 `demo-bff --check-config` 可只校验配置与密钥，不连接数据库或 discovery；正常启动 discovery/密钥/数据库失败时不提供业务端口。

## 接口与身份流程

| 接口 | 用途 |
|---|---|
| GET /bff/login | 创建五分钟一次性流程并跳身份平台 |
| GET /bff/callback | 消费匹配本浏览器和 namespace 的 state/flow，创建应用会话 |
| GET /bff/session | 实时身份状态检查；需要时共享锁内刷新 |
| GET /bff/csrf | 返回绑定本应用随机 Cookie 的 HMAC CSRF |
| POST /bff/logout | 本应用退出，并尝试撤销本应用授权 |
| POST /bff/identity-logout | 本应用退出，再跳身份平台确认当前身份退出 |

`GET /bff/login` 用可失败的 CSPRNG 生成 flow Cookie/state/nonce/verifier，OIDC 库计算 PKCE S256 和授权 URL。PG 保存 flow Cookie/state 摘要及加密 nonce/verifier。回调只接收单值 state/code 或协议 error，必须匹配本浏览器与本 namespace，一次消费后不重用。维护中的 `openid=0.25.0` SDK 交换 code 并验证 RS256 签名、issuer/audience/nonce/expiry。BFF 补充精确 kid、未来 iat、最长五分钟 ID Token 生命周期、UUID sub/sid、auth_time 及必要实际 amr；刷新后的 ID Token 不含原 nonce。依赖审查见 [dependency-review.md](evidence/T13/dependency-review.md)。

创建 BFF session 后跳首页，浏览器仅得到随机 HttpOnly Cookie。`GET /bff/session` 每次真实 introspection，不缓存 active=true；除 active 之外还校验完整 sub/client_id/exp/iat/scope/token_type。明确 inactive 时提交应用 session 失效并清 Cookie；异常 schema、超时或依赖错误返回 503，页面不显示已登录内容，不用历史验签结果继续授权。

access 临近到期时，PG 共享 session 行锁覆盖一次 SDK refresh 和新令牌加密提交。并发等待者读取最新记录，跨进程不能重复使用旧 refresh。刷新响应必须轮换 refresh，验证后的 sub/sid/auth_time/amr 保持稳定且绝对到期不延长。invalid_grant 返回 401；其他刷新错误返回 503；两者都先提交应用 session 失效、清 Cookie，不无限重试旧 token。数据库完全不可用时可能无法提交失效标记，只能 503 失败关闭；恢复后旧 refresh 重用由身份平台 replay 检测撤销，不能假定曾刷新成功。

两个退出 POST 都要求精确 Origin、单值 `X-CSRF-Token`，且不接收请求体。本应用退出先取得共享行锁、保存内存中临时 refresh、提交本地 session 失效，然后用服务端 Basic 向固定 `/oauth/revoke` 发一次有超时的撤销请求。正常响应返回 `revocation_status=revoked`；故障返回 `failed`，仍清应用 Cookie，页面明确“本应用已退出，平台授权撤销暂未完成”。不存在有效本地会话时返回 `not_required`。本应用撤销只影响 A 或 B 自己的授权，IdP 身份及其他应用会话继续保留。

平台退出先执行上述本地步骤，再跳固定 `ISSUER/oauth/logout`，不带 ID Token hint。由浏览器 IdP Cookie 和身份平台确认页面撤销当前身份会话，其他 BFF 的下一次保护请求立即检查到失效。当前设备退出不声称退出所有设备；退出全部设备需使用身份平台对应明确动作。无 hint 不请求外部回跳，也不把 ID Token 放入浏览器 URL/HTML。

## 撤销故障、运行和验收边界

撤销请求失败后本地会话已经失效，不能恢复为已登录或重复使用旧 refresh；服务不把失败称为成功。当前实现没有后台重试队列，返回 `failed` 后会擦除临时秘密。需要撤销剩余授权时，用户可在身份平台撤销对应应用授权，或明确退出当前/全部身份会话；接入方若另建后台重试，必须持久加密任务、定义过期和幂等边界，不能把 token 发给浏览器重试。IdP 确认请求失败时保留确认页的错误与重试，本地退出依然成立。

HTTP 连接超时三秒、总请求超时十秒，无重定向。JWKS 仅公开 RSA/RS256/sig，唯一 kid、至少 2048 位 modulus、65537 exponent，拒绝私钥及外部密钥引用。验证失败仅重取固定公开 JWKS 并再验同一响应，不重发已消费 code/refresh；新公钥在该次验证使用，长期共享公钥缓存更新留后续运维任务。SDK 原错误/Debug 不写日志；配置和 token 容器在应用可控生命周期擦除。SQL statement logging 禁用。SIGINT/SIGTERM 等待进行中请求结束。

业务权限仍由应用自己的服务规则判断，本演示只返回当前身份 UUID。T13 集成/E2E 必须真实验证 A 登录后 B 复用 IdP 身份但仍首次同意、退出全部后 A/B 下次拒绝、浏览器存储/网络/日志无 OAuth 秘密、并发刷新仅一次、introspection 与撤销故障失败关闭；state 篡改不能创建应用会话。测试用隔离 schema 和临时配置，认证秘密不持久化到 trace/截图，结果以 [T13 实际证据](evidence/T13/) 为准。
