# T06 当前执行环境

日期：2026-10-08；基线提交7f736ac，完整测试编排提交2fce25e。Linux/WSL2、Rust1.98.0、Node22.22.1；实际PostgreSQL17.11、Redis7.4.11、Mailpit1.31.4；Chromium153。测试使用identity_test临时schema，仅清理本轮schema；不写数据库URL/秘密。

用户明确回复生产域名、主机、SMTP及独立备份暂未准备。继续独立代码/开发验收，T22/T23实际生产相关条件保持未验收。上一次GitHub CI Linuxcheck成功，Windowsunit和T03integration失败，日志未取得，不能宣称远端CI绿；后续本地及CI修复记录另记。
