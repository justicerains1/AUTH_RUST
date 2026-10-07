# T09 Passkey 实现与验收边界

2026-10-08。实现webauthn-rs0.5.5库验证、固定RP/origin/UV、注册resident策略/credProps、加密服务器挑战、最多十凭证、本人管理、使用时间与签名cursor、强认证登录/重新认证和安全通知。库MPL-2.0维护及源码核查记录，缓存RustSec319依赖0漏洞。state只服务器加密存储，不接受客户端提供库状态。

真实PG策略集成通过近期认证/加密挑战/归属/十个限制，CDP虚拟CTAP2两完整流程真实通过注册/无allowlistdiscoverable登录/Passkey强认证修改密码。fresh尚未消费签名分别改变origin/challenge/signature被拒，原合法签名随后成功；消费后重放拒。导出虚拟私钥只内存、重新签名UV=false的真实有效proof仍被库requiredUV拒，原fresh签名仍成功，避免签名损坏或已消费掩盖UV检查。所有密钥/断言/秘密不写证据、trace或截图。

标准clientDataJSON名称与真实库extensions.uvm schema字段先前导致失败，已精准修复，不任意放宽schema。同步计数由库update处理Somefalse可接受，不一概要求递增。Check、单元、构建及相关集成/E2E均0；失败记录保留。

T09-PK-04真实手机/桌面认证器及Safari未提供，保持阻塞。任务整体待验收，不能仅凭虚拟认证器声明整个T09通过。后续T10独立前置T06/T02已满足，可继续完成不依赖设备的代码；发布仍受全部必要验收约束。
