# 本轮SQL证据摘要扫描复核

Gitleaks实际发现两个generic-api-key命中：sql-probe-2026-10-09T00-28-49-567Z.json第4617行tokens.rs源码SHA和第4622行apiExecutableSha256。前者与当前真实源码逐字节SHA相同；后者由受测runner读取当次进程/proc/pid/exe计算，是公开完整性摘要，后续重新编译的target二进制不同不能称该旧SHA仍为当前binary。两值不属于密码/token/客户端秘密。

只按真实提交57aa52c、文件、规则、行号的两个fingerprint加入.gitleaksignore，无目录或规则级豁免。原实际运行报告及摘要保留；后续完整扫描单独执行，新发现不能自动豁免。

最终full中的sql-probe-2026-10-09T03-16-00-882Z同两行命中，源码摘要集合与先前已审报告逐项相同，实际执行程序SHA也相同；仅新增实际提交0a25d99的两个精确fingerprint。最终报告提交后再次完整Git历史扫描通过，未用秘密值替换校验数据。
