# T05 完整回归失败与独立复测

2026-10-08 UTC（上海时间2026-10-09）。完整回归原T05接口集成18:06:44.361～18:07:05.150退出1，子Rust退出101；当时runner仅返回固定安全错误，没有保存子进程诊断，所以无法确定实际失败断言。原失败已另存 [integration-full-2026-10-08T18-06-44-failure.txt](integration-full-2026-10-08T18-06-44-failure.txt)，不以随后局部通过覆盖原完整结果。

只修改 [T05 runner](../../../tests/integration/T05.mjs) 的诊断保存：Rust/startup/browser/cleanup失败分别生成带时间且`wx`防覆盖的脱敏文件。读已有环境、生成AEAD/private key和当前测试密码仅在内存做精确替换；移除Cookie/Authorization/私钥/Token、邮件fragment与query秘密、email/UUID、完整密码字节数组。已有认证断言、时限、Worker重试与生产源码未变。Node语法、ESLint、diff检查实际0；通过当前函数的合成诊断验证确认具体“left=503/right=200”保留，秘密/password bytes/Cookie/链接值被删除。

三次独立真实T05接口/PG/Redis/SMTP复测均退出0，全部原断言（注册/Worker送达、GET不消费、明确确认/单次消费、重发旧新动作、枚举响应、负向/audit rollback、实际Mailpit停启/outbox重试）执行：

| 独立运行 | UTC开始～结束 | 结果 |
|---|---|---|
| 第一次 | 18:38:23.175～18:38:41.504 | 退出0，原T05套件全执行。 |
| [第二次](integration-repeat-2-2026-10-09.txt) | 18:46:46.219～18:47:04.047 | 退出0，原T05套件全执行。 |
| [第三次](integration-repeat-3-2026-10-09.txt) | 18:47:49.095～18:48:06.828 | 退出0，原T05套件全执行。 |

复测未出现失败，所以新Rust安全诊断没有触发，不虚构具体原因。邮件选择或SMTP恢复时机仅是源码审查候选，未据此修改fixture、放宽断言或宣称缺陷已解决。当前原完整回归仍失败；最终新提交完整运行由根另行记录。三次复测后实际确认Mailpit running/healthy，未停止PG/Redis，故障窗口已释放。

剩余限制：此次未定位的偶发失败需要下一次新诊断或完整回归结果进一步确认。局部复测通过不等于生产SMTP或系统发布通过。
