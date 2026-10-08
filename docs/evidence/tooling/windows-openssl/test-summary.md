# Windows OpenSSL 构建前置修复

2026-10-08，父39f5b83。新增脱敏CIannotation实际获得job113005025521错误：Windows MSVC openssl-sys0.9.117无法检测OpenSSL安装。不是推断编译或认证实现错误。

根据实际官方runner清单及安装脚本，windows-2025预装完整OpenSSL3.6.4。新增PowerShellsetup严格验证版本、headers、MSVC import libs及DLL，指定动态链接目录与PATH；不删Windows检查、不更换认证库、不下载未审核安装器。来源/许可与blob见infra/windows/README.md。

本地Linux没有PowerShell和MSVC，静态审查完成；Windows真实check/unit/build需本提交远端执行，当前待验收。若版本或目录不符脚本必须非零失败，失败证据保留。
