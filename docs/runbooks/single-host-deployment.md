# T22 单机发布与回滚操作稿

这里是可审查的部署制品与步骤，当前没有真实生产域名、DNS/主机、SMTP/独立备份环境，T20/T21未放行时不能据此开始生产发布或将T22标通过。单机维护存在中断，不声明高可用。

## 制品与配置

`infra/Dockerfile.prod`构建locked Rust release，runtime Debian12.15+OpenSSL3.0.22/CA/curl，应用UID10001；`infra/Dockerfile.edge.prod`构建三React应用并由Caddy2.11.7服务，UID10001监听容器8080/8443。`infra/compose.prod.yaml`仅edge发布主机80/443，PG/Redis无公开端口。应用read_only/cap_drop/资源与PID限制，tmpfs只/tmp；Caddy证书状态仅/data,/config持久卷。

将镜像推送/验证后填写`infra/production.env.example`对应的不可变发布digest，不使用浮动latest。实际机密配置不入Git；放`.local/production/identity.env`、`demo-a.env`、`demo-b.env`及`secrets/`。identity.env明确APP_ENV=production、固定HTTPS ISSUER、匹配RP_ID、PG/Redis URL、current签名kid/key文件、AEAD版本文件/activekid、SMTP required及用户名/秘密文件、TRUSTED_PROXY_CIDRS；不打开seed/debug。BFF的PUBLIC_ORIGIN/clientID/secret/namespace/Cookie各不相同，Cookie以__Host-开头，生产不设置BFF_ISSUER_CONNECT_HOST。

秘密文件绑定RO到/run/secrets；宿主文件应`chown 10001:10001`及`chmod 0600`，父目录仅运维账户可进入。Config拒绝group/other权限，不能把文件改0640来迁就容器读取。PG密码文件由Postgres入口读取，另设置其所需权限，数据库服务权限不和API私钥混用。私钥与AEAD另独立受控备份。

发布runtime包含`identity-keys`维护命令；生产使用受控配置及明确`--allow-production`执行重加密。独立存储`BACKUP_DESTINATION`、公开`AGE_RECIPIENT`以及PG归档脚本/已核验age工具挂载必须准备，Compose的5分钟WAL切换与后续10分钟归档预算见[backup-recovery.md](backup-recovery.md)。基础备份每日调度、14天可恢复链保留、告警与完整恢复尚须外部系统配置实测。

API和Worker的内部指标配置`METRICS_TOKEN_FILE=/run/secrets/metrics-token`，使用至少256位随机token的受限文件，和Prometheus的bearer_token_file对应。该token不作为账号或OAuth凭证，指标只有固定聚合标签；Caddy不向公网代理/metrics。未配置token时仅loopback可读，不能误以为私网采集已可用。告警规则及配置见infra/ops，生产可达接收端需实际配置演练。

未配置旧JWKS时准备仅`{"keys":[]}`的public文件，不能含私钥。生产Compose绑定所有秘密文件必须预先存在，不自动生成或覆盖生产密钥。正式issuer由DNS/证书指向edge；外部TCP80只TLS证书挑战与HTTPS跳转，HTTPS443提供身份/应用。运维SSH独立受控，不把DB/Redis映射公网。

## 发布顺序

1. 完整基础验收与外部条件放行后，检查生产配置及秘密权限，分别运行镜像`identity-server --check-config`/`identity-worker --check-config`及BFF配置校验；失败停止，不输出秘密。
2. 独立加密存储备份与WAL归档状态确认，实际备份成功才继续；记录版本和校验。
3. 显式维护命令运行`identity-migrate --allow-production`，迁移失败非零立即停止发布，不启动新应用或自动降库。
4. 启动指定不可变镜像；readiness检查PG/Redis，Worker队列状态、edgeTLS/安全headers和BFF页面/API冒烟。
5. 真实注册→验证→密码/MFA/Passkey→A/B SSO→退出全部→禁用检查；监控成功再完成放行。首次发布不能用Mailpit代替真实SMTP/DNS送达。

不在容器启动命令中自动迁移，独立maintenance profile只供明确操作。Compose stop/down默认保留卷；不可逆schema变更单独窗口与备份恢复方案。

## 可执行编排

运维准备一个绝对路径、0600、当前用户所有的JSON配置后，运行`node infra/ops/release.mjs deploy /absolute/release-config.json --allow-production`。生产只接受`repository@sha256:<64hex>`镜像，不接受浮动tag；本地专属演练必须`mode=local-review`并使用`--local-review`。脚本不会生成生产配置/秘密，也不会代替上面的发布关卡。

配置字段为`mode`、`project`、`release`、`rustImage`、`edgeImage`、`composeFile`、`envFile`、`stateDirectory`、`backupDirectory`以及`backup`/`facts`/`smoke`三个受控命令。每个命令必须`{"program":"/absolute/executable","args":["literal","arguments"]}`，使用参数数组而不执行shell文本。stateDirectory必须当前用户0700，保存版本/阶段及0600诊断；秘密不要放命令参数，继承受控文件路径。`backup`要实际调用base-backup.sh及独立存储，脚本核对本次新`last-success.json`、密文大小/SHA/manifest，零退出但旧回执或损坏对象仍停止。

`facts`可调用`node /absolute/infra/ops/release.mjs facts /absolute/release-config.json --allow-production`，配置另加`databaseUser`、`databaseName`和`encryptionKeysFile`。它在真实Compose PG查询SQLx迁移版本/校验/成功记录并输出摘要，查询TOTP/outbox/挑战/BFF所有密文字段的kid集合，受控key文件只取配置版本名，绝不输出key值。`facts`命令同样是显式绝对program+args；不得用预制JSON或手写true代替实际数据库查询。`smoke`应是已经审查的真实业务冒烟程序；health不代替注册/邮件/MFA/Passkey/双SSO/退出/禁用全链。

编排固定：Compose配置验证→镜像存在→API/Worker/A-B配置检查→实际schema/key事实→新备份成功且验证→独立迁移成功→启动→readiness→smoke。迁移失败非零时不执行up、不变更成功current记录；启动后失败也保留failed事件，不能静默宣布完成。脚本从不自动回滚数据库、停止数据服务或删除卷；中断后查看受控事件再按恢复方案处理。

回滚命令为`node infra/ops/release.mjs rollback /absolute/release-config.json --allow-production`，还须`compatibilityFile`受控文件。内容必须明确`version=1`、`from_release`/`to_release`、记录的`old_rust_image`/`old_edge_image`、审查允许的当前数据库`accepted_migration_sha256`列表、旧版可读取的`old_supported_kids`以及`schema_compatible=true`/`keys_compatible=true`及具体reason。脚本将真实当前迁移摘要/在用kid与这些范围比对；超出范围停止，布尔值本身不足以放行。旧镜像必须存在且通过配置校验，再做本次备份才切换旧应用/健康/smoke，**不执行down migration**。兼容文件须有上版实际验证依据，不能把脚本比对当未验证schema兼容的证明。

`node --test tests/ops/release-order.test.mjs`只验证编排拒绝/顺序；`node tests/ops/release-local.mjs`另启动任务独有内网PG/Redis，实际验证旧/新应用发布、坏迁移中止与兼容回滚。后者无主机80/443、没有生产部署，结果见[T22本地记录](../evidence/T22/release-orchestration-summary.md)。

## 回滚和恢复要求

保留前版应用/edge镜像digest，schema兼容时切换前版镜像后重新readiness与冒烟，不自动down migration。schema不兼容则按已审查恢复方案，说明维护中断。每日基础备份+持续WAL到独立加密存储，≥14天；新主机恢复DB/签名/AEAD/配置并检查账户/TOTP/Passkey/OAuth/撤销，实测RPO≤15m/RTO≤60m才可标达标。

签名轮换先发布新公钥，再切私钥kid，旧公钥≥12h+2m；AEAD版本保留旧key解密再后台重加密。告警必须覆盖5xx/延迟/连接池/攻击/队列/磁盘/证书/备份；生产SMTP设置SPF/DKIM/DMARC并实际检查送达与失败通知。上述恢复、轮换、监控、DNS/SMTP是后续真实验收项，本稿不宣称已执行。
