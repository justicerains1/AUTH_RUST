# ADR-0002：以权威状态检查实现提交后即时撤销

- 状态：接受。
- 日期：2026-10-07。
- 关联：T02.03/T02.07、T03/T06/T12/T13/T14/T20/T24。
- 验收：T12-REV-01、T12-REV-05、T13-BFF-02、T14-ADM-04、T24-HA-02；E15/E16/E18/E25。

## 背景与决定

用户已确定撤销提交后立即影响下一次认证检查。RFC 7009 §2.1 说明立即失效并指出传播窗口应最小；RFC 7662 §2.2/§4 允许缓存但明确牺牲状态鲜活性。本系统选择不透明 access/refresh 和 PostgreSQL 联合权威状态检查，不缓存 active=true。ID Token 是身份声明，不能作为被撤销后的永久业务请求凭证。

每次 introspection 验证 Basic client 及 token归属，并在一个当前可见的参数化查询中检查 user、session、grant、client 与 token。撤销 COMMIT 成功为 C；其后发起查询 Q 的检查必须看见无效状态，不使用旧长事务快照、滞后副本或正缓存。Q<C 且已通过检查的远端业务请求允许完成；平台不承诺回滚业务动作。

用户禁用、密码变化、全部退出在同事务撤销全session/派生授权；当前退出和单grant撤销保持其各自范围。code/refresh/new session事务按用户→session→grant→token/action/challenge统一加锁和状态重验，防止禁用/退出竞争产生新有效凭证。refresh已用摘要重放时，整family撤销与审计必须提交，再返回invalid_grant。

BFF每次保护请求introspection，明确inactive清应用session；查询失败返回503并拒绝请求。共享应用session锁串行refresh，跨实例不只进程mutex。Redis只处理限流/辅助状态；PG认证状态失效失败关闭，Redis限流入口失效失败关闭。

## 取舍与验证

每请求查询增加数据库负载；T21按300RPS/15分钟及混合流真实验证索引、连接池与容量，不降安全规则换吞吐。可缓存公钥等非撤销授权事实，不能缓存有效token状态。允许失效结果缓存不是实现必要项；避免新增复杂缓存策略。

T12/T20用同步屏障控制refresh/logout、login/disable、撤销后新query；数据库状态和提交时间为证据。T24跨实例重复验证。当前只是架构决策，未运行这些认证竞争案例。来源阅读证据见 [spec-sources.md](../evidence/T02/spec-sources.md)。
