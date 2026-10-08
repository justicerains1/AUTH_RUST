# Caddy 静态资源缓存修复

实际本地HTTPS制品验证发现，common_security里的`-Server`让整个header处理器延迟到响应写入时执行，最终把资产路径已经设置的`public, max-age=31536000, immutable`覆盖成`no-store`。静态文件与hash正确，但浏览器不能按设计使用内容哈希文件长期缓存，原Caddy配置检查没有验证这个行为。

只把删除Server头拆成独立header指令，保留HTML/认证/协议入口no-store与全部CSP/HSTS规则。实际候选配置测试三站HTML、资产及HTTP跳转12项通过：HTML no-store、内容哈希资产immutable、CSP/HSTS和成功静态响应无Server。缺后端时API/OAuth/BFF仍502且no-store，不冒充后端健康；错误响应头边界单独记录。

验证使用相同生产routes加专属测试证书/本地TLS，未申请正式域名或发布生产。候选实际报告见[T23缓存验证](../T23/final-artifacts-20261009-cache-fix.json)。正式提交后将从提交归档重新构建edge并复验，runtime代码未因这一修复改变。
