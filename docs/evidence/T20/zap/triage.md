# 本地 ZAP 初轮结果与范围

ZAP2.17.0首次运行器两个参数错误（Integer大小写及未预载URL）非零，报告保留。第三轮anonymous/authenticated spider+API active扫描实际完成，高危/严重0；public SPA Vite测试页缺CSP/anti-clickjacking/nosniff导致中低告警，这是开发服务器，不能作为生产头部验证。

改为本地已构建生产静态资源和固定身份API代理，响应策略与已审Caddy生产配置一致，再次扫描；没有攻击生产或忽略对应规则。API HTML错误页实际CSP未定义form-action（无默认fallback），已明确补self表单目标及完整资源策略。初轮所有报告保留，复测不能自动抹除残留风险。

ZAP公开JSON只包含规则/风险/置信/path/描述/修复建议，不保存原请求响应、Cookie、JWT或body。临时登录由测试库真实passwordsession创建，扫描Cookie仅私有API替换规则且结束删除；测试schema清理。ZAP本机loopback5299禁APIkey仅受控本地，不部署此daemon。
