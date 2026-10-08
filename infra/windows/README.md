# Windows MSVC OpenSSL 前置

`openssl-sys 0.9.117`在Windows MSVC默认无法根据PATH推断完整头文件/库目录。这里使用官方`windows-2025`已安装的OpenSSL full package，精确核对3.6.4，不下载未知安装器或改用未审核依赖。

实际官方来源查询：

- [Windows2025-Readme](https://github.com/actions/runner-images/blob/main/images/windows/Windows2025-Readme.md)：OpenSSL3.6.4、VCPKG_INSTALLATION_ROOT=C:\vcpkg。
- [Install-OpenSSL.ps1](https://github.com/actions/runner-images/blob/main/images/windows/scripts/build/Install-OpenSSL.ps1)：非light、INTEL64，安装`$ProgramFiles\OpenSSL`，copytobin并加bin到PATH，验证include存在。
- [Tools.Tests.ps1](https://github.com/actions/runner-images/blob/main/images/windows/scripts/tests/Tools.Tests.ps1)：OpenSSL命令应来自ProgramFiles\OpenSSL\bin，完整include目录存在。

2026-10-08实际通过GitHub contents API读取上述文件，文档blob SHA为`c38b0de6889c4261061e6f954454c2375aaece29`，安装脚本blob SHA为`21518b01b310f5abe055c05242d7c7ce8a462ea9`。源码库OpenSSL3使用Apache-2.0；已有Rust openssl0.10.81同许可，不新增Rust依赖。

[setup-openssl.ps1](setup-openssl.ps1)检查准确版本、ssl.h/opensslv.h、同目录libssl.lib/libcrypto.lib，以及bin的OpenSSL3运行DLL。首选`lib\VC\x64\MD`，再检查明确候选目录；找不到则显示实际库布局并非零失败，不静默继续。验证后设置OPENSSL_INCLUDE_DIR/OPENSSL_LIB_DIR/OPENSSL_STATIC=0、public PATH，写入GITHUB_ENV/GITHUB_PATH供后续check/unit/build使用。

CI需在Rust setup之后、首次Cargo命令之前加入：

```yaml
- name: Configure Windows OpenSSL
  if: runner.os == 'Windows'
  shell: pwsh
  run: ./infra/windows/setup-openssl.ps1
```

本地64位Windows可运行同脚本或显式传已安装目录。GitHub runner未来OpenSSL版本变化会失败并要求更新已审核ExpectedVersion；不能为了通过检查省略头文件/库/DLL验证。当前Linux环境没有pwsh，未执行Windows脚本；最终成功必须由Windows CI真实check/unit/build结果证明。
