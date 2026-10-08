# 生产 Compose tmpfs 配置修复

真实本地发布演练发现 `tmpfs: [/tmp:size=16m,mode=1777]` 在 YAML flow 列表中被解析为两个挂载项，Docker 启动时拒绝非绝对路径 `mode=1777`。早期 `compose config` 和单独 `docker run --tmpfs` 没有证明生产 Compose 可启动。

生产 Rust 模板和 edge 均改为包含一个完整字符串的列表 `['/tmp:size=16m,mode=1777']`，保持16MiB、1777、非root/只读和cap-drop限制。实际模型测试在 maintenance profile 也加入后，逐一验证 API、Worker、migrate、双 BFF 和edge只有这一完整挂载，退出0。首次测试忽略maintenance而没有migrate服务，测试自身TypeError非零保留本地诊断，补profile后通过。

根文件修复与专属本地Compose发布/回滚演练分别记录，后者真实启动相同挂载语法及权限约束；没有启动生产80/443或写入真实生产环境。最后完整回归仍按实际提交执行。
