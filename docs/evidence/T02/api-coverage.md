# T02 API逐路由覆盖

本表由OpenAPI契约的operation逐项形成，仅记录契约覆盖，不表示端点已实现。真实OpenAPI解析/schema校验由 `npm run verify:openapi` 输出补充。

| 方法 | 完整路径 | operationId | 认证等级 | 实现任务 |
|---|---|---|---|---|
| GET | `/api/v1/auth/csrf` | `getCsrf` | public | T04 |
| POST | `/api/v1/auth/register` | `register` | preauth | T05 |
| POST | `/api/v1/auth/email-verification/request` | `requestEmailVerification` | preauth | T05 |
| POST | `/api/v1/auth/password-reset/request` | `requestPasswordReset` | preauth | T07 |
| POST | `/api/v1/auth/email-verification/confirm` | `confirmEmailVerification` | preauth | T05 |
| POST | `/api/v1/auth/login/password` | `passwordLogin` | preauth | T06 |
| POST | `/api/v1/auth/mfa/totp/verify` | `verifyTotp` | preauth | T08 |
| POST | `/api/v1/auth/mfa/recovery/verify` | `verifyRecovery` | preauth | T08 |
| POST | `/api/v1/auth/password-reset/confirm` | `confirmPasswordReset` | preauth | T07 |
| POST | `/api/v1/me/reauth/password` | `reauthPassword` | authenticated | T08 |
| POST | `/api/v1/me/reauth/totp` | `reauthTotp` | authenticated | T08 |
| POST | `/api/v1/auth/logout` | `logout` | preauth | T06 |
| POST | `/api/v1/auth/logout-all` | `logoutAll` | preauth | T06 |
| GET | `/api/v1/me` | `getMe` | authenticated | T06 |
| POST | `/api/v1/me/password/change` | `changePassword` | recent-auth | T07 |
| GET | `/api/v1/me/sessions` | `listSessions` | authenticated | T06 |
| DELETE | `/api/v1/me/sessions/{id}` | `revokeSession` | authenticated | T06 |
| POST | `/api/v1/me/mfa/totp/enrollment` | `beginTotpEnrollment` | recent-auth | T08 |
| POST | `/api/v1/me/mfa/totp/enrollment/confirm` | `confirmTotpEnrollment` | recent-auth | T08 |
| DELETE | `/api/v1/me/mfa/totp` | `disableTotp` | strong-auth | T08 |
| POST | `/api/v1/me/mfa/recovery-codes/regenerate` | `regenerateRecoveryCodes` | strong-auth | T08 |
| POST | `/api/v1/me/passkeys/registration/options` | `beginPasskeyRegistration` | recent-auth | T09 |
| POST | `/api/v1/me/passkeys/registration/verify` | `verifyPasskeyRegistration` | recent-auth | T09 |
| GET | `/api/v1/me/passkeys` | `listPasskeys` | authenticated | T09 |
| GET | `/api/v1/me/passkeys/{id}` | `getPasskey` | authenticated | T09 |
| PATCH | `/api/v1/me/passkeys/{id}` | `renamePasskey` | authenticated | T09 |
| DELETE | `/api/v1/me/passkeys/{id}` | `deletePasskey` | strong-auth | T09 |
| POST | `/api/v1/auth/passkeys/options` | `beginPasskeyLogin` | preauth | T09 |
| POST | `/api/v1/auth/passkeys/verify` | `verifyPasskeyLogin` | preauth | T09 |
| POST | `/api/v1/me/reauth/passkeys/options` | `beginPasskeyReauth` | authenticated | T09 |
| POST | `/api/v1/me/reauth/passkeys/verify` | `verifyPasskeyReauth` | authenticated | T09 |
| GET | `/api/v1/oauth/transactions/{id}` | `getAuthorizationTransaction` | preauth | T10 |
| POST | `/api/v1/oauth/transactions/{id}/decision` | `decideAuthorization` | authenticated | T10 |
| GET | `/api/v1/me/grants` | `listGrants` | authenticated | T12 |
| DELETE | `/api/v1/me/grants/{id}` | `revokeGrant` | authenticated | T12 |
| GET | `/api/v1/admin/users` | `adminListUsers` | admin | T14 |
| GET | `/api/v1/admin/users/{id}` | `adminGetUser` | admin | T14 |
| PATCH | `/api/v1/admin/users/{id}/status` | `adminSetUserStatus` | admin | T14 |
| POST | `/api/v1/admin/users/{id}/revoke-sessions` | `adminRevokeUserSessions` | admin | T14 |
| GET | `/api/v1/admin/clients` | `adminListClients` | admin | T14 |
| POST | `/api/v1/admin/clients` | `adminCreateClient` | admin | T14 |
| GET | `/api/v1/admin/clients/{id}` | `adminGetClient` | admin | T14 |
| PATCH | `/api/v1/admin/clients/{id}` | `adminUpdateClient` | admin | T14 |
| POST | `/api/v1/admin/clients/{id}/rotate-secret` | `adminRotateClientSecret` | admin | T14 |
| GET | `/api/v1/admin/members` | `adminListMembers` | admin | T14 |
| POST | `/api/v1/admin/members` | `adminAddMember` | admin | T14 |
| DELETE | `/api/v1/admin/members/{user_id}` | `adminRemoveMember` | admin | T14 |
| GET | `/api/v1/admin/audit-events` | `adminListAuditEvents` | admin | T14 |
| GET | `/.well-known/openid-configuration` | `getDiscovery` | public | T11 |
| GET | `/oauth/jwks` | `getJwks` | public | T11 |
| GET | `/oauth/authorize` | `authorizeGet` | public | T10 |
| POST | `/oauth/authorize` | `authorizePost` | public | T10 |
| POST | `/oauth/token` | `exchangeToken` | oidc-client | T11 |
| GET | `/oauth/userinfo` | `getUserInfo` | authenticated | T11 |
| POST | `/oauth/introspect` | `introspectToken` | oidc-client | T12 |
| POST | `/oauth/revoke` | `revokeToken` | oidc-client | T12 |
| GET | `/oauth/logout` | `beginRpLogout` | preauth | T12 |
| POST | `/oauth/logout` | `beginRpLogoutPost` | preauth | T12 |
| POST | `/oauth/logout/confirm` | `confirmRpLogout` | preauth | T12 |

JSON操作 48 项；标准协议操作 11 项。所有operation均明确x-auth-level/x-auth-policy、CSRF、幂等与限流策略，错误码HTTP映射，request/response与示例；无任意object或空schema替代WebAuthn证明结构。

## 实际解析与校验

执行日期：2026-10-07，Node v22.22.1，npm 9.2.0；工作目录仓库根。

- `npm run verify:openapi`：退出0。真实 Swagger Parser/OpenAPI3.1结构与引用解析，加 Ajv2020 schema/example 和逐路由/认证/CSRF/分页/状态/协议契约检查；输出59操作、48 JSON API、125个含内联schema检查、598个去重schema/operation example检查。该计数包含内联schema与参数，因此不同于命名组件数。
- 独立 `SwaggerParser.validate("docs/api/openapi.yaml")`：退出0，OpenAPI3.1.0、59操作。
- Python jsonschema4.19.2 Draft202012+FormatChecker：105命名schema及581个命名/operation示例验证，0错误。此项交叉校验，不替代根脚本。
- 首次独立解析曾发现info.license只含name不符合该解析器要求的identifier/url，已移除尚未确定的license对象；修复后复测通过，未删除接口/schema校验。

上述验证仅证明契约可解析、引用/schema/示例与路由范围一致，没有执行尚未实现的认证端点。协议交互、安全负向与数据库原子消费需由对应T04～T14后续真实案例验收。

最终协议复核同步：authorize未验证回调的400/429/503是身份域安全HTML+明确字符串schema，已验证回调才302返回标准error/state；根校验仅对此规范分支允许HTML，其他协议错误仍严格标准JSON。未知token_type_hint按RFC7009/7662忽略，只有实际不能撤销类型才unsupported_token_type。退出Cookie OR绑定无会话幂等204不产生他人副作用。复核后verify:openapi与verify:docs均退出0。
