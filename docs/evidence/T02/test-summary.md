# T02 真实文档与工具验收

实际核查规范、状态机、威胁矩阵、三个ADR和每个API。OpenAPI3.1实际59operations（48 JSON、11协议）、125schema及598唯一example验证通过。SwaggerParser13.1.0处理本地引用及结构，Ajv8.20.0/ajv-formats3.0.1验证2020-12 schema与实例；三依赖MIT、当前维护及兼容Node22已核查，npm audit0。

34威胁关联任务/验收/E均存在；六状态机、强认证、用户→session→grant→action锁序与COMMIT后检查撤销边界人工核对。实际9次规范HTTP200含版本/日期SHA256/关键段落读取证据。

工具59项测试通过（含坏ref/schema/example、遗漏路由/错误码/认证级别/CSRF/Basic form/分页/作用域）；check与两verify命令退出0。首次validator3.1开放schema未拦非法type，补Ajv；HTML授权错误误套JSON、reauth错误分支及计数重复问题均修复并保留失败输出。文档修正已有GET/POST协议入口、Passkey列表/confirm路径、锁序和未知hint处理。认证业务端点尚未实现，不能把契约验收当功能验收。
