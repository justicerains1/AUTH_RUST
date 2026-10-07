# T04 安全基础实际验收

新增8个core安全测试以及原9测试均通过：ASCII邮箱边界、Unicode15/128/512byte密码、固定MIT弱清单校验、真实Argon2id64MiB/3/p1与旧PHC编码参数验证、共享dummy与4任务预算250ms、CSPRNG/常量比较、RFC4231 HMAC、版本化AESGCM全部篡改及旧key兼容。来源许可/版本/sha记录见dependency-source.md及data metadata。

真实HTTP/PG/Redis中间件测试验证CSRF/Origin/Cookie/重复字段/请求上限/代理白名单、10并发原子预算与Retry-After、邮件目标与IP预算区别、固定preauth绝对期限及CSRF轮换、当前session权威状态、审计失败同事务rollback、实际停止Redis/PG时503并恢复。临时security fixture只是检查边界，未实现密码登录、不生产开放测试路线。

unit、check、build、verify、tooling与tasksecurity均退出0。在线RustSec重试成功270dependencies/0vulnerabilities/warnings{}，初始networkfetch、编译接口和Dockerfile未copydata失败保留。服务启动补copydata后重新构建，不通过删除失败证据掩盖。后续T05/T06真实业务需继续验收；全量security仍未完成，不放行生产。
