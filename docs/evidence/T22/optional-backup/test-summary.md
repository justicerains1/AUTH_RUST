# 安装备份目录可选挂载

用户明确要求不强制挂载。安装器允许受控普通绝对目录，默认/var/backups，记录backupStorage=local-directory；已挂载目录记录mounted/backupMount，不把mountpoint退出1作为安装失败。普通目录同机故障风险仅提示，不需额外确认才能安装。仍创建wal/base、核对设备和权限、加密/校验备份、迁移前要求成功备份。

后续backup仅对已选择挂载的目录做mountpoint检查；所有类型都核对原设备号，避免错设备或挂载丢失写入。原backupMount配置保持兼容，升级不会默默取消既有保护。

22测试全部0：新增普通目录可接受/无mandatory mount与已挂载后丢失拒绝；原真实隔离PG备份验证/加密解密、最新release选择、代理/安装/CI发布顺序测试保留。Node ESLint/bash/docs/OpenAPI/diff检查0。完整新服务器安装仍未实测；允许普通目录不等于独立故障域、十四天恢复历史或生产RPO-RTO达标，原发布关卡仍未放行。
