# T22 age 归档加密工具来源

实际读取官方 release API https://api.github.com/repos/FiloSottile/age/releases/latest ：v1.3.2，发布于2026-08-29T17:55:36Z。Linuxamd64 asset官方digest `sha256:cbe24006683f8eb669266162894b9a522a1af52f2665fbc63a4bb032ed26ac10`；固定版本LICENSE为BSD-3-Clause，源码license SHA256 `c5d65279d02955c0fc2294ae417c3add650d228f4f5c82bd6d531fc26c89cd96`。

直接release下载读超时失败，随后通过GitHub asset API `Accept: application/octet-stream`实际取得19405817字节，计算SHA256与官方digest一致。临时提取age/age-keygen，`age --version`实际输出v1.3.2；未复制第三方二进制到仓库。

归档/恢复shell语法检查退出0；实际单文件加密/幂等/拒损坏及独立PostgreSQL PITR见 [local-pitr-drill.md](local-pitr-drill.md)。未取得生产独立存储或运行每日保留调度，不标生产验收通过。

最终回归把已核验的两个可执行文件复制到忽略目录 `.local/security-tools/age/`，供 T22 完整集成默认使用。`age` 的 SHA256 为 `eb7dd1b518f0a307c99cd97782623c5321da049154b04acd2d98d21aa7bc9b2c`，`age-keygen` 为 `0a0009db842259d6717f7eeb30acb6b90d2a2eb924c6acd0a0db0ca1f1537899`；可用 `AGE_BINARY`/`AGE_KEYGEN_BINARY` 明确指定已核验位置。干净机器需先取得官方固定归档、核对上方归档摘要后提取这两个文件，设置可执行权限；不提交工具或密钥。缺工具将使真实运维测试非零失败。
