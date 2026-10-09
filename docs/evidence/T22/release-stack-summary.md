# T22 五服务发布、迁移中止与兼容回滚

2026-10-09 UTC。[release-stack-1791513932161.json](release-stack-1791513932161.json)记录02:42:30.649～02:45:32.161的完整独立演练，实际退出0。新 [release-stack.mjs](../../../tests/ops/release-stack.mjs)沿用生产 [release.mjs](../../../infra/ops/release.mjs)编排默认五服务：API、Worker、demo A/B BFF、edge；新演练已接入T22集成必跑入口，缺镜像、服务或真实smoke失败不得通过。

本次明确是`APP_ENV=test`与`local-review`，使用专属PG/Redis/Mailpit、固定HTTPS测试issuer/RP、Secure/__Host Cookie、非root/read-only/cap-drop/no-new-privileges、只loopback高端口。edge本地CA证书由本次生成，Node按CA验证、BFF受控系统根bundle、Chromium临时NSS信任并保持`ignoreHTTPSErrors=false`；未关闭证书检查。真实邮件投递为测试Mailpit明文SMTP，不能称生产SMTP STARTTLS/DNS送达。

## 制品与执行顺序

| 真实镜像 | 配置摘要 / 源码版本 |
|---|---|
| 旧runtime `auth-rust-runtime:t23-2c9c712` | `sha256:6080ddcd99ea92b53c750e11c206c7781a6cd4261d3c5cd515b8af75f4d07f9c`，revision2c9c712。 |
| 新runtime `auth-rust-runtime:t23-df28706` | `sha256:49d77f0d7944407b3eb5321a820b441f5bc54e19514ff2494f79e2e92a328ef4`，revisiondf28706。 |
| edge `auth-rust-edge:t23-df28706` | `sha256:73e1c6aded1bd777b7d5a9aa99ef5d2c17163ccd6799528110a23569f249314b`，revisiondf28706。 |

摘要为实际image inspect的配置/镜像ID，不把它混称OCI index。runtime旧/新及edge各固定镜像实际存在；脚本九个源文件/锁摘要在运行前后完全一致，记录sourceCommit，不把后续修改当作已运行。没有重用开发私钥、数据库或共享服务。

准备阶段在专属DB执行明确迁移、真实stdin bootstrap密码哈希，首次绑定管理员实际TOTP并用强认证API创建两客户端；这为后续BFF discovery准备必要注册，未SQL设置strong_at/伪造身份。此后正式编排执行：

- 旧版发布、新版发布各15阶段：Compose配置→镜像存在→四服务实际`--check-config`→真实migration/kid facts→本次实际pg_basebackup/pg_verifybackup/age加密及回执/manifest/SHA验证→迁移→五服务up→真实readiness→业务smoke。
- 在此专属DB破坏SQLx checksum后，实际migrator非零中止。五个已运行容器ID均未更换、current仍记new；没有up/ready/smoke。恢复测试checksum后migration摘要与原值一致。
- 回滚比对实际当前migration摘要/在用kid与保留旧镜像兼容文件，另做本次新备份，再五服务切回旧runtime、readiness和完整业务smoke。13阶段通过，没有down migration。兼容仅针对这两个明确镜像、相同schema/key map，不推定任意版本可回滚。

## 真实业务冒烟

[smoke程序](../../../tests/ops/release-stack-smoke.mjs)在旧、新、回滚后三个阶段分别实际完成：注册→专属Worker SMTP投递→从Mailpit读取本账号fragment token并POST验证→密码登录/近期密码→Chromium虚拟CTAP2注册、退出及实际签名登录→真实TOTP绑定及新时间步密码MFA登录→A/B分别同意SSO并真实BFF /session200→平台全部退出后A/B下一检查401→真实强认证管理员禁用该用户后新密码登录401。

上述不是只检查容器healthy。edge headers、安全策略、API readiness经严格CA的实际HTTPS请求验证；A/B BFF discovery及请求也实际经TLS完成。虚拟认证器仅自动化，不替代实体设备。此次用户在管理员禁用前已经全部退出，所以只证明**禁用后新登录拒绝**；禁用旧活跃session/access/refresh的组合另见T14 E16，不声称此stack重复覆盖。

## 失败保留与清理

首轮edge未等待ready导致拒绝连接；后续实际调查发现Docker内部网络端口虽然Compose声明却未向host生效，增加专属edge/Mailpit ingress网络且仅loopback发布，数据PG/Redis仍private。TLS始终严格，未去掉证书/秘密控制。五服务recreate时BFF首次discovery失败，是测试Compose遗漏生产已有`restart: unless-stopped`；补忠实配置后真实ready/smoke通过，未改生产发布脚本或BFF重试逻辑。所有此前`release-stack-*.json`失败与脱敏日志保留。

结束只down此任务Compose/删除专属卷与目录，临时CA唯一nickname删除；实际额外确认无release-stack容器/CA nickname残留。任一CA/容器清理失败均整体非零。公开证据只阶段、镜像/备份校验、兼容摘要及安全错误，不含密码、Cookie、token、因子种子或私钥。Node语法/ESLint/diff检查通过。

该演练补齐T22.04/.05完整五服务的本地可复查行为；正式域名/TLS运营、生产SMTP/DNS、独立存储、兼容发布审查及生产smoke/24小时高峰仍需原关卡。生产四服务配置校验与此testmode真实运行是不同范围，不把本地CA/测试SMTP当生产认证。
