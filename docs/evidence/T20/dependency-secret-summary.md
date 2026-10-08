# T20.01 依赖、许可证与秘密检查子模块

2026-10-08。Gitleaks8.30.1和cargo-deny0.20.2从正式GitHub release下载，SHA256与官方checksums一致，版本/来源见scan-tool-versions.json。完整Git历史首次7告警经逐行复核为用途字符串、文件校验和及无效示例，精确指纹分诊后真实扫描退出0、无剩余发现，未忽略路径或规则。

cargo-deny --locked --offline实际检查完整传递依赖许可证与来源退出0。允许已核查permissive许可、MPL-2.0、CDLA-Permissive-2.0等实际依赖；MPL库修改与分发需保留其文件许可/源码义务。仅private first-party workspace缺许可证声明不作第三方扫描错误，所有第三方仍扫描。来源仅crates.io，未知registry/git拒。RustSec缓存扫描见最近任务（366依赖0），在线刷新未由此替代。

已安装npm第三方锁包读取许可声明，平台不兼容的optional包按锁中的精确版本向npm registry读取许可元数据，不把未安装标成已运行。npm-audit在线实际报告0漏洞，完整清单保存。源码workspace链接为first-party不混入第三方清单。许可证声明核查不自动放行将来的依赖、产品分发或全部安全验收。ZAP、完整威胁/并发/故障/互操作仍在T20后续，当前只子模块，不标T20通过。

实际npm锁清单306个第三方包，其中23个平台optional包通过精确registry元数据核对：许可包含MIT/Apache/ISC/BSD/MPL/CC0/0BSD，另Python-2.0、CC-BY-4.0、BlueOak-1.0.0必须随分发保留适用声明/署名。没有把未安装平台包声称执行过。工具license Gitleaks为MIT、cargo-deny为MIT或Apache2，release源码未替代产品许可决定。
