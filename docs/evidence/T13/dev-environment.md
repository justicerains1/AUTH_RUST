# T13 本地双 BFF 开发环境

开发数据库identity_development已按检查和迁移命令应用0007；dev:clients真实注册A/B，第二次仅复核已注册回调/secret摘要不轮换，两secret文件0600、目录0700。明确production及其他数据库拒，秘密未回显。开发setup是显式CLI，API启动不隐式创建或迁移。

Compose的bff profile运行同一个Rust包两个独立实例，distinct client、callback、Cookie/namespace与secretfile；身份前端保留原公开issuer。开发DNSoverride仅identity-web固定内部主机，保留请求Host/URL/iss；production拒此override。默认开发启动保持原服务，BFF使用显式profile，需先migrate/dev:clients。

首轮真实BFF启动拒绝discovery：Vite未代理/.well-known导致返回HTML；增加准确单route代理后10服务全部healthy，A/B /bff/session匿名401，公开JSONdiscovery issuer正确。此失败证明SDK拒绝HTML，未用伪metadata。详细实际输出dev-compose-failure.txt/dev-compose.txt/dev-http.txt。

RustSec no-fetch实际364依赖0漏洞warnings空；缓存公告库HEAD沿用此前记录，不声明在线更新成功。最终模块SSO/并发/故障由独立T13 harness验收。

根工程回归T01使用当前应用title/Reactroot验收，真实默认服务及PG/Redis停机恢复退出0；旧初始化文案不作为产品功能长期契约。两平台远端CI截至T12在a499520全部配置项通过（run37711498834），T13后需新的CI结果。

开发CLI注册与重复复核前后不轮换secret；若注册后写文件失败，CLI明确失败并保留空文件防无意覆盖，需人工受控清理/客户端轮换修复，不能把错误标成功。CLI不提供生产seed。
