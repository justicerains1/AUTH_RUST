# T13 双 BFF SSO、共享刷新与故障验收

2026-10-08，父a499520；Linux/Rust1.98/Node22、真实PG17/Redis7.4、Chromium153。4个必要案例已真实通过，代码版本由本模块Git提交记录。

同一Rust demo-bff包运行A/B及第二A实例，独立client/secret/callback/Cookie/namespace；长期OAuth令牌AEAD加密，flowcookie/state摘要及加密nonce/PKCE持久化五分钟。维护中openid0.25.0 SDK真实discovery/PKCE/Basic/code交换及RS256/iss/aud/nonce/exp验签；补精确kid/未来iat/UUIDsub/sid/实际amr（包含rcv）与刷新声明稳定，拒未知公钥/私钥字段。SDK源码/许可/MSRV见dependency-review.md；锁文件缓存RustSec364依赖0漏洞、无警告，不标在线更新成功。

真实4项浏览器/API组合：A登录后B复用IdP身份，B首次同意；IdP全设备退出后两BFF下次拒；十请求跨A/A2共享PG行锁仅一次refresh；真实Redis停机时状态检查503，恢复正常，需刷新故障也503且清会话。状态查询无active正缓存，异常true声明拒绝。浏览器存储/响应无OAuth令牌或secret，不发送/oauth/token。

本应用退出先提交BFF失效，再服务端一次refreshrevoke：A授权真实撤销，B不受影响；撤销故障返回failed仍清Cookie，无后台队列。平台退出固定无hintIdP地址，经浏览器自身IdPCookie确认，不暴露IDToken。真实callbackstate篡改拒创建session。秘密/trace未持久化。

本地开发10服务真实healthy，显式dev:clients创建/重复复核0、600权限，productionsetup拒，固定devDNSoverride+精确metadata代理保持原issuer。首轮缺metadata代理SDK拒HTML，修后真实成功。T01工程及依赖停机恢复回归0；A/B UI共12测试、身份40、Rust全部单元、check/build/docs/openapi及65工具测试0。最后rcv兼容一行修复后BFF6单元与Clippy0。详细integration.txt/test-boundaries.md/final-check.txt/unit.txt/build.txt/dev-*.txt。

早期失败保留：scope fixture缺profile、测试共用consent、导航取消responsebody Promise、平台退出浏览器上下文、13个expect_used检查失败、同期依赖停机；分别精确修fixture/测试隔离/Promise等待/Result错误处理，未放宽安全检查。T14草稿未导出且依赖隔离，不混入本模块。

截至T12的远端a499520/run37711498834 Linux/Windows构建单元与集成全部成功；T13本提交远端结果待查询，仍不宣称全量T20～T23或生产/实体设备验收通过。


最终回归修复：Redis恢复需实际PONG；原组合故障案例总60秒失败，拆成独立故障后T13六案例真实通过，全部原断言/默认时限/生产请求预算保留，修复f5e7640。[失败与复测](../tooling/t13-fault-isolation/test-summary.md)；最终整套回归另记录，不以局部通过替代失败报告。
