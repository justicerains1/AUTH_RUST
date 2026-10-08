# T19 后台查询子模块验收

2026-10-08，父e374809。真实T14管理员CLI/API套件包含新增过滤断言，退出0；email/status精确返回匹配用户，未知status/重复参数拒，原cursor变更过滤拒；审计from/to成对最多31天，合法当前窗口有真实事件，缺端点/过宽窗口拒。测试代码保留在t14_admin.rs，不以UIfixture代替数据库。

identity-store/server lib/bins Clippy退出0，docs/OpenAPI验证退出0。新过滤摘要纳入已有签名cursor route绑定，SQL参数化；未重命名接口或允许任意SQL。T19UI未完成，此处只对应T19.01/.05必需服务端子模块。
