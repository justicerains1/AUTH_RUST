# T22 本地隔离 PITR 演练

2026-10-08，仅两个任务专属临时容器 `auth-rust-restore-test-primary` / `auth-rust-restore-test-recovered`，使用固定PostgreSQL17.11镜像。network none、没有主机端口；未停止或修改开发PG/Redis，也不代表生产或独立主机演练。

真实执行pg_basebackup（plain、stream WAL）及pg_verifybackup成功。基础tar使用age1.3.2 public recipient加密，WAL经archive_command调用仓库archive-wal.sh，写age加密对象与完整性校验。恢复解密到新空PGDATA，配置restore_command、named recovery target与recovery_target_action=promote，启动新容器。

探针表三步：基础备份前行1、备份后恢复点前行2、恢复点后行3。恢复至`t22_target`后数据库已promote、行1/2存在、行3不存在，证明真实WAL重放到指定点，而不只是导入逻辑dump。

本次恢复操作开始到目标业务探针校验完成计时 **1.903秒**。指定目标点之前的探针数据丢失为 **0秒**；这是小样本受控命名恢复点的本地结果，不推定实际事故RPO≤15m或生产RTO≤60m。未验证账户/TOTP/Passkey/撤销完整恢复、每日调度/14天保留、独立存储/私钥备份或告警送达。

同日无PG单文件归档测试实际成功：同文件重复归档幂等、解密内容一致、不同原文覆盖拒绝、损坏密文拒绝、非法WAL名拒绝、解密失败不产生目标文件。该测试是加密归档边界验证，不称为PostgreSQL演练；上述真实PITR另行证明。

两临时容器与自动卷均已删除，cleanup命令退出0；只清任务创建资源。本地加密备份/测试身份仍在Git忽略的.local/t22-drill，未提交明文WAL/数据或密钥。
