# T16 实际结果

用户已确认T15方案，实际基础组件与公开/账号/管理路由、TanStack Query/CSRF API client、Radix Dialog/Input、React Hook Form/Zod控件、中文字典及分包已落地。业务登录页面在T17继续。

根check/build/unit成功，身份前端17项Vitest（11UI+6API）通过；transport/guard mock仅纯前端状态测试，不冒充真实身份服务。Chromium153/Playwright1.63/axe4.13实际开发组件页360/390/768/1440axe0violations/无横溢出/44px目标，键盘Dialog初始cancel/loop/ESC返回、标签/错误、passwordtoggle/清秘密留邮箱、cursor分页边界及200%textscale通过。截图人工查看手机布局。正式应用跨浏览器/读屏与性能仍按T20/T23。

浏览器脚本初次exact文本定位忽略状态图标导致等待失败，改role和hasText后通过，失败报告保留。错误清理时表单提示被resetField移除已修keepError；CSRF失败不会自动重复危险POST。

npm run test:accessibility现会启动本地5190专用Vite，真实浏览器检查后停止；缺浏览器失败。T16_BASE_URL只允许localhost/127.0.0.1开发地址。production构建不含devcomponents路由/chunk。
