# T14 管理员权限、初始化与事务

实现 [admin.rs](../crates/identity-store/src/admin.rs) 与 [CLI](../crates/identity-admin-cli/src/main.rs)。所有后台方法独立验证当前会话、verified/active/credential_version、enabled成员与近期strong_at；无成员或未配置强因素返回Forbidden，有因素但强认证过期返回StrongRequired，正常权限不足不假称依赖故障。首个未绑定因素管理员只允许既有受限因素绑定流程，后台列表/变更不开放。

CLI命令为 `identity-admin-cli bootstrap --email <address>`，密码来自TTY隐藏输入或单行stdin；不接受密码参数或环境密码。stdin最多512 UTF8字节/128Unicode，允许完整密码加一次CRLF，错误/超长输入在Zeroizing中清理且不打印实际值。测试隔离旗标`--test-schema identity_test_<suffix>`只允许APP_ENV=test、actual database identity_test和安全schema名，连接前验证，不在production开放。

Bootstrap只能接收AdminStore.verify_bootstrap通过真实密码验证/新密码hash得到的私有VerifiedBootstrap。已有用户必须verified/active、密码正确并在最终用户锁下版本一致，不覆盖hash、不启用账号。新首管理员是受控本地初始化，创建verified账号但无因素仍binding_only。全局数据库advisory锁在用户锁前，确认无任何管理员成员才初始化；并发首次初始化只有一个提交，后续拒绝。

所有管理员变更先全局保护锁，按UUID锁actor/目标users，再权威验证actor会话。后续授予只针对已有合格verified/active且TOTP或Passkey的用户；未资格及不存在目标为TargetIneligible，重复enabled成员StateConflict。删除/禁用最后可用管理员被拒；尚未绑定因素的唯一首管理员同样不能被禁用/删除。只有明确LastAdministrator错误提交拒绝审计，不把数据库错误记成权限拒绝。

用户禁用/全部会话撤销按user→session→grant→token锁序，状态与派生失效及审计同事务。目标不存在返回404；全部撤销计数只统计原来未撤销、未到期会话，重复返回0。审计失败整个高风险变更回滚。成员移除保留普通用户session，但下一次后台检查失去membership权限。

TOTP关闭与Passkey删除共用相同全局admin保护锁，必须在user锁前获取；模拟删除后如该管理员无剩余因素且无其他合格可用管理员则拒，避免通过因素删除绕过最后管理员规则。普通因素查看/登录不新增此全局锁。

客户端创建使用服务端随机安全client_id与256位secret，摘要入库，秘密只创建/轮换响应一次。更新精确login/logout回调与有限scope，固定HTTPS/开发loopback规则；停用时当前token状态因client enabled=false立即失效，secret轮换使旧Basic认证失效。当前契约不把停用等价永久撤grant：重新启用后未过期且其他状态有效的旧grant可恢复，后续安全审查需据明确产品政策决定永久撤销。scope变更不伪造已有同意，授权新请求检查当前允许scope。

用户/客户端/成员/审计列表只有固定排序cursor/limit，不接受未定义过滤字段；HMACcursor绑定actor/route/排序/十分钟期限，默认HTTP20/max100，SQL参数化。审计返回真实事件字典和HMAC脱敏来源，不映射不存在的别名、无密码/token/请求体。

CLI边界单测真实通过、store/CLI Clippy通过；真实初始化并发、后台权限、最后管理员/因素删除、审计rollback与旧凭证失效由T14集成证据核对，最终验收以acceptance记录为准。
