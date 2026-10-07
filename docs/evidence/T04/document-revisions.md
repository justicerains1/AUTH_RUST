# T04 数据及安全测试校正

- 原7.1缺少可持久单次预认证CSRF与浏览器流程绑定实体，T04按已有信任边界补preauthentication_contexts表，仅存256位cookie摘要/CSRF摘要/期限。PG是权威，不使用Redis active缓存绕过撤销。
- 根test:security新增可选--task=Txx选择已实施安全套件；全量未全部实现仍明确非零。已有T20/T23全量关卡保留，不以局部通过放行发布。
