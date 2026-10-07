# T08 二维码依赖核查

执行日期：2026-10-08（Asia/Shanghai），Node22.22.1/npm9.2.0。

使用精确 `qrcode-generator@2.0.4`，许可证MIT，官方仓库 <https://github.com/kazuhikoarase/qrcode-generator>；npm registry time.modified为2025-08-07T14:11:54.823Z，未声明deprecated，无运行时依赖，自带 `dist/qrcode.d.ts`，同时导出ESM/CJS。

引入前读取 `npm view qrcode-generator version time.modified license repository.url engines dependencies --json` 与精确版本types/exports。同类qrcode1.5.4引入PNG/yargs等本场景不用的依赖；此实现仅使用二维码矩阵，保持SVG与页面原生代码。

TOTP二维码组件按路由/动态import懒加载。二维码库只计算布尔矩阵，React原生SVG rect/path绘制，不使用innerHTML。提供可读aria-label，并同时提供手动设置密钥。二维码URI、种子与恢复码只在当前组件内存；取消/完成时卸载清理，禁止browser storage、下载秘密图片、遥测或证据截图。用户主动复制恢复码使用剪贴板，提示及时在可信位置保存与清理。

精确依赖由根安装并更新锁文件；本模块实际 `npm audit --json`退出0：0漏洞，info/low/moderate/high/critical均0（309依赖元数据）。类型检查及ESLint退出0；当前身份前端6文件29行为测试通过。真实TOTP/恢复码与设备认证由对应API、数据库与浏览器验收，不用二维码计算成功冒充身份认证。
