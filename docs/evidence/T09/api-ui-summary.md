# T09 Passkey HTTP 与页面模块记录

日期：2026-10-08（Asia/Shanghai）。本模块实现计划中的注册、discoverable登录、重新认证、本人列表/详情、改名和删除HTTP入口及最小真实页面。库验证与PG事务由独立核心/仓储模块实现；本记录不把外部真实设备验收标为通过。

## 实现与信任边界

- AppState使用固定Config issuer/RP_ID创建webAuthn-rs服务与统一Clock；不能从Host、return_to或用户输入决定RP/origin。
- 注册options/verify、登录options/verify、本人reauthoptions/verify、GET列表/详情、PATCH名称、DELETE凭证沿OpenAPI路径。状态变更Origin/CSRF与真实Redis挑战/IP预算；凭证请求嵌套DTO拒未知/重复字段，严格id/rawId一致和base64url长度形状，库再验证CBOR、签名、challenge、RP、origin、UV。
- 注册要求residentKey/UV，客户端credProps.rk只为非签名提示，不宣称硬件保护。公开options与服务库最终接受算法限定ES256/RS256；ID Token RS256规则不被混为WebAuthn算法。
- assertion登录由已存credential识别用户，事务复核verified/active/version和purpose/当前预认证或session绑定，消费挑战、更新库凭证状态、创建强会话或更新strong_at共同提交。amr使用user，不伪声明hwk；同步counter未变化的Some(false)合法，不强迫递增。
- 本人凭证最多10个，改名仅名称，不允许越权；删除需近期强认证并保留有效密码登录方法。列表default20/max100、签名cursor与真实last_used_at，server不忽略分页参数。

浏览器Zod与DOM转换明确challenge/user.id/credential ids→ArrayBuffer，attestation/clientDataJSON/authenticatorData/signature/userHandle→base64url；不用any或任意object绕校验。options/响应只保留本profile定义字段与真实库所需扩展，未知信任字段拒绝。

页面支持注册名称、设备注册、discoverable登录、Passkey强认证、本人改名/删除；浏览器不支持或用户取消时保留密码与第二因素回退，不制造成功或自动重放。只有账号已有Passkey时显示相应强认证入口。错误保留request_id并区分429/503状态，秘密和凭证不进browser storage或遥测。

## 实际验证

- 前端TypeScript strict/ESLint退出0；Vitest8文件33项通过。新增3项字节与信任参数转换、未知/降低UV拒绝；1项用户取消不调用verify或制造强认证成功。fixture只用于前端行为，不冒充认证。
- server check与Clippy all-targets --locked -D warnings退出0；新增strictDTO回归验证标准clientDataJSON拼写。相关模块格式检查完成，其他并行任务的暂态格式结果由根整合核对。
- 前端production build退出0，各页面与WebAuthn辅助代码按route拆包；未下载远程JS或保存秘密图片。
- [真实策略集成](integration.txt)退出0：注册近期认证保护、加密服务器状态、本人/他人凭证删除与10个限制；没有fabricated WebAuthn成功。
- [真实CDP E2E](e2e.txt)2例通过：虚拟CTAP2认证器生成实际签名，注册、discoverable登录、reauth与拒绝边界。它是实际库/浏览器验证，不代替外部手机/安全密钥体验。

首轮注册422因Serde camelCase把client_data_json映为clientDataJson；增加明确clientDataJSON标准名称并保持deny_unknown_fields。随后登录options被严格schema拒绝，定位库默认extensions.uvm=true，按具体字段同步OpenAPI、Zod与DOM输入，没有放宽任意扩展；原失败证据保留。复测两例通过。

T09-PK-04外部真实手机/桌面认证器尚未提供。其注册/登录/取消/密码回退不能以虚拟认证器证据替代，T09整体保持待验收，最终状态与推送由根整合记录。
