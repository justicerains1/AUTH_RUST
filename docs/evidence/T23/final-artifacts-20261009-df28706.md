# 管理批量查询与WAL指标修改后的实际生产制品

从真实已提交 `df28706a70118404ba60d3ca994863d271067f84` git archive 到任务独立目录，按Cargo/npm锁重建runtime和edge；[双镜像实际验证](final-artifacts-20261009-df28706-both.json)退出0，36阶段与54制品摘要全部记录。包含新admin批量查询和Worker真实WAL指标，不沿用旧runtime输入等价冒新实现。

| 本地tag | OCI index | 实际源码 |
| --- | --- | --- |
| `auth-rust-runtime:t23-df28706` | `sha256:49d77f0d7944407b3eb5321a820b441f5bc54e19514ff2494f79e2e92a328ef4` | df28706a70118404ba60d3ca994863d271067f84 |
| `auth-rust-edge:t23-df28706` | `sha256:73e1c6aded1bd777b7d5a9aa99ef5d2c17163ccd6799528110a23569f249314b` | df28706a70118404ba60d3ca994863d271067f84 |

[源码归档清单](runtime-df28706/prepared-source.json)、[真实构建与OCI元数据](runtime-df28706/build-result.json)、[runtime编译日志](runtime-df28706/build.txt)、[edge构建日志](runtime-df28706/edge-build.txt)公开留证。使用现有BuildKit cache，runtime相关生产源码实际重新编译；没有声称全空缓存。第一次手录revision label错误，实际归档源码始终df28706，核对后[正确revision缓存重建](runtime-df28706/build-label-corrected.txt)并检查inspect为真实40位SHA；最终两镜像revision均正确，错误初值不当完成证据。

权限验证实际UID10001、read-only、cap-drop ALL、NoNewPrivs1；六运行/维护binary、秘密0600拒不安全权限、RS256只公开字段、Caddy能力清除及原HTTPS配置均通过。真实本地CA三个站点TLS验证，HTML/JS摘要等于镜像，HTML no-store、hash JS immutable、HSTS/CSP/nosniff/Server去除均通过。没有生产域名/ACME/SMTP或全业务readiness；五服务业务发布/回滚另由本地stack演练覆盖，不从静态页面推定认证正确。

先前[只runtime新build+旧edge验证](final-artifacts-20261009-df28706.json)保留；[edge输入对照](edge-input-equivalence-df28706.json)仅root package新增test:interactions脚本差，app源码/依赖锁不变。最终仍实际重建edge来明确版本制品，不依赖等价说明替代。后续测试/运维文档新增不参与release binaries，但最终完整/干净检出须与新提交独立核对。

生产独立部署、正式邮件/监控、参考机器、设备和24小时观察未验收，本地tag/校验不代表注册表发布或正式上线。
