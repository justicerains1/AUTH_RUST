# 最新成品与公开下载

脚本默认按Release published_at选最新已完整uploaded的安装版（install.sh/部署包/SHA全有），跳draft/旧版缺安装器/上传不全。包校验和与manifest照常核对，不回退源码编译。仓库公开，改用browser_download_url，不询问GitHubToken，也不向下载网关发送Authorization。

连接超时/总deadline/重试加入API、部署包、Node和Docker key下载；交互可填HTTP/HTTPS代理和可信HTTPSrelease文件网关。Docker daemon代理单独询问是否配置并重启，不假定shell代理会作用daemon；文件网关不代理GitHub API/GHCR。未预置不明加速域名或关闭TLS/哈希校验。国内机器真实连通性仍需目标机执行，不以本机成功保证。

[20测试](tests.txt)全部0；新增直接执行脚本jq选择表达式的fixture，证明发布时间排序、草稿/旧不带安装器/缺SHA跳过。首次本机缺jq真实失败，安装运行依赖后测试0；安装脚本自身已安装jq，不跳缺工具。bash/ESLint/docs/OpenAPI/diff0。

[实际公开访问](result.json)：当前可安装最新Release2870891，runtime/edge匿名GHCRtoken与manifest均200，现有成品可无需登录拉取。将来新package须保持Public，repoPublic不能单独证明packagePublic。结果对应查询时点，不预告本次新commit已经发布。
