# T16 前端组件与可访问路由实现记录

执行日期：2026-10-07，Linux，Node22.22.1/npm9.2.0。范围T16.01/.02/.04/.05/.06/.07/.08；API/CSRF/Query客户端由T16.03并行模块提供。

## 已实现范围

- 已采纳T15的暖白/近黑/冷蓝、控件边界色、8/16px圆角、中文系统字体、1280px容器、420px表单及移动20px边距；品牌为“统一身份中心”。
- Button、Input、Password、FieldError、Status、Radix ConfirmDialog、Table、CursorPagination、Empty，明确禁用/加载/错误/空/不可用状态。
- 首页、登录基础入口、账号与后台保护路由、未找到边界，按route lazy拆包；页面标题、跳主要内容链接、路由转换焦点、autocomplete和aria-live。
- `/dev/components`仅`import.meta.env.DEV`开发可用；production构建没有ComponentsPage、Radix表单展示chunk。此页只测试本地控件，不提交认证，不改变账号。
- AuthGuard真实接APIClient `/me`，401回登录；503/网络失败保留可重试状态；403普通拒绝；普通账号不能因已登录进入后台；管理员必须已绑定因素且强认证不足5分钟。前端guard仅改善体验，后端权限必须独立检查。
- 密码/验证码只在当前控件内存；示例本地格式检查后清除秘密，失败保留错误文本和非秘密邮箱；不写browser storage。T17完整注册/登录/MFA/找回流程尚未实现，不用示例表单冒充认证。

## 实际命令结果

| 命令 | 退出码 | 结果 |
|---|---|---|
| `npm run check --workspace @identity/identity-web` | 0 | TypeScript strict通过 |
| `node node_modules/eslint/bin/eslint.js apps/identity-web --max-warnings 0` | 0 | 整应用严格ESLint，0 warning |
| `npm run test:unit --workspace @identity/identity-web` | 0 | 3个文件、17个测试全通过 |
| `npm run build --workspace @identity/identity-web` | 0 | production按路由拆包，开发gallery未进入产物 |

17项测试包括：6项基础组件（label/粘贴/错误关联、密码显示隐藏不提交且保留输入、加载与禁用防重复、Dialog取消焦点限制/Escape返回、Dialog操作失败不暴露内部错误、Table加载/空/失败区分）；5项route（首页品牌/标题/焦点、404、401回登录、503恢复、普通账号拒后台）；6项API client行为（独立T16.03模块）。

route中401/503与普通账号示例由明确API spy fixture驱动，验证前端错误和权限提示；API transport单元测试使用可控制fetch。它们不是实际服务器认证成功，也不标记后端T04～T14接口验收。组件Dialog成功只关闭本地示例，没有账号操作。

## 浏览器反馈与正式验收边界

本模块临时使用既有Chromium1208与Vite55173进行开发自检：360/390/768/1440全部无横向溢出，axe WCAG2/2.1/2.2 A/AA均0；Dialog初始取消和Escape关闭焦点返回通过；三个无效字段aria-invalid均true；390px下200%字体缩放无横向溢出。临时Vite与浏览器均已关闭；此项仅开发反馈。

根集成验收使用Playwright1.63与其固定Chromium153，负责真实gallery axe、键盘/对话框、四尺寸及200%缩放，输出由相应浏览器证据记录；不可将临时旧浏览器结果替代正式固定浏览器验收。真实后端认证尚未实现，不宣称账号登录或后台授权成功。
