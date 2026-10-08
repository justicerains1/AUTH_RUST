# Caddy 与生产权限约束的修复

2026-10-08 UTC，最终镜像审计基于 `319f30c`。上游 Caddy 2.11.7 二进制带有 `cap_net_bind_service=ep`。在真实生产约束 `--read-only --cap-drop ALL --security-opt no-new-privileges` 下执行失败，退出 255，提示 `operation not permitted`；仅检查只读和 UID 的历史测试没有覆盖这组约束。

生产容器监听 8080/8443，由 Docker 映射外部 80/443，进程无需低端口绑定能力。`infra/Dockerfile.edge.prod` 在构建时明确移除 Caddy 文件 capability，保留非 root、只读、删除全部 capability 和禁止权限提升。

修后锁文件镜像构建退出 0；同组权限限制和 `--network none` 下 Caddy 配置 validate、version 检查退出 0，UID10001，`getcap` 无文件能力，静态目录不可写。最终提交对应制品及摘要见随后生成的 T23 final-artifacts 证据；没有生产部署或正式证书申请。
