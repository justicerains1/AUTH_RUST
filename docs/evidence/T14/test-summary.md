# T14 管理员初始化、权限与事务验收

2026-10-08，父f44afd7。Linux/Rust1.98/Node22、真实PG17/Redis7.4；模块四个必要案例实际通过，代码版本由本模块Git提交记录。

两个实际CLI进程用stdin并发bootstrap，仅一个初始化，再次拒绝、密码不回显；已有账号真实密码验证后私有能力值参与最终版本复核，新账号受控初始化，TTY隐藏读取或单行stdin最多512UTF8/128Unicode（含完整CRLF）。测试schema仅APP_ENV=test/identity_test生成白名单，生产不支持。rpassword7.5.4精确Apache2/MSRV核查记录admin-source.md。

首个无因素管理员只绑定必要流程，普通用户后台403。实际密码近期认证+TOTP enrollment及独立验证码确认后访问13管理端点，后台再次独立查身份/当前membership/factor/strong；强认证过期提供标准next。后续授予只verified/active有因素已有用户，客户端随机secret只创建/轮换展示一次，旧Basic立即失效。

全局admin锁先于user锁，删除/禁用最后管理员拒并提交denied审计；自助删除最后TOTP/Passkey共相同锁，两真实合格管理员并发各删因素仅一人成功且保留一人。用户禁用撤全部会话/派生授权，客户端停用状态即时无效，强制审计失败变更全部回滚。不存在revoke-sessions目标404，首撤count1重复0。分页cursor绑定actor/route/期限，无任意SQL/filter。

真实CLI/API integration退出0；T08/T09改动回归退出0、T09实体设备待验收。check、Clippy最终、unit、build、docs/OpenAPI与65工具测试均0，前端52项。最终integration.txt/clippy-final.txt/final-check.txt/unit.txt/build.txt及test-boundaries.md可复查；详细事务见[说明](../../admin-transactions.md)。缓存RustSec366依赖0漏洞无警告，lastcommit/update空，未宣称在线更新。

初期编译/fmt与空body测试误发送{}已修，不放宽emptybody/强认证/审计策略，脱敏失败保留。契约审计enum同步真实安全字典，ADMIN强认证采用既有AUTH_REAUTH_REQUIRED标准next，删除最后因素409ADMIN_LAST_MEMBER；见contract-revisions.md。

当前客户端停用只即时enabled权威拒；重新启用可恢复原未过期有效grant，这是本次明确行为边界，永久撤销通过revoke/授权页完成。管理UI在T19接入，完整安全/性能/生产/真实设备发布尚未放行。此前f44afd7远端Linux/Windows全部CI成功（run37718112029），T14本提交CI待实际结果，不以本地通过代替。
