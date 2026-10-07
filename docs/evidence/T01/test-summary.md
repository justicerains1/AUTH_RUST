# T01 验收结果

## 实际行为

八个开发服务均 healthy，三个空路由 HTTP 200；API live/ready 为 200，身份前端 health 代理同源可达。production 有效配置返回 0，缺秘密、非 HTTPS/带路径 issuer、RP 不匹配、弱 Cookie、明文 SMTP、无效密钥和开发开关等全部非零，错误仅包含配置名。分别停止 Redis/Postgres 时 live 200、ready 503，恢复后 ready 200。详见 integration.txt。

Rust 配置 8 项测试，三前端各 2 项真实路由测试，Node tooling 32 项通过；fmt/Clippy/TS/ESLint、Rust release 和三前端 production 构建成功。npm audit 0，RustSec 238 dependencies、0 vulnerabilities、warnings 无。

## 失败及修复

- 同时增加 serde 直接依赖而尚未同步 Cargo.lock：locked 构建拒绝；补齐现有版本依赖关系后成功，无版本漂移。
- Compose 多服务同时输出相同 image tag：BuildKit 导出冲突；每服务独立 tag，共享构建缓存，复测成功。
- 配置审查发现原始 issuer 归一化、公钥整数/SMTP/重复 AEAD/非 UTF-8 回显：增加严格校验及拒绝测试，复测成功。非 UTF-8 修复证据单独保存。
- 集成脚本误认为 Compose images -q 含 sha256 前缀、start 支持 --wait：改用 inspect Image ID、start 后轮询真实健康；恢复过程提前重跑的 starting 失败也保留，服务健康后全套通过。
- T15 的证据采集程序浏览器全局被 ESLint 拒绝：明确 globalThis 后通过。

失败原始报告保留，禁止删除或把失败写成通过。当前目录没有 Git，未触发远端 CI；Windows/macOS/生产运行没有执行。T01 通过仅限当前真实 Linux/Compose 工程基线，不表示后续认证任务完成。
