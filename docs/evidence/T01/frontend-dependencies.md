# T01 前端依赖与验证证据

记录时间：2026-10-07，Asia/Shanghai。执行环境：Linux，Node v22.22.1，npm 9.2.0。对应实现范围：T01.01、T01.02、T01.03。

## 依赖核查

引入前使用 npm registry 的正式包元数据核查维护、许可证和 Node 兼容要求；没有 `deprecated` 返回字段。表内 `time.modified` 是整个包的 registry 更新时间，不是所选版本发布日期。它与项目官方仓库来源共同用于维护核查；安全公告通过完整锁文件的 `npm audit` 查询 npm advisory 数据库。

核查命令模式：

```sh
npm view <package>@<exact-version> version time.modified license repository.url engines deprecated --json
npm view typescript-eslint@8.71.1 peerDependencies --json
npm view @vitejs/plugin-react@6.1.2 peerDependencies peerDependenciesMeta --json
npm view vitest@5.0.3 peerDependencies --json
```

| 精确依赖 | 许可证 | registry 更新时间（UTC） | Node engine |
|---|---|---|---|
| `@eslint/js@10.0.1` | MIT | 2026-07-10T20:16:17.404Z | `^20.19.0 || ^22.13.0 || >=24` |
| `@types/node@22.20.5` | MIT | 2026-10-01T22:39:33.908Z | `未声明` |
| `@types/react@19.3.0` | MIT | 2026-10-04T09:18:58.831Z | `未声明` |
| `@types/react-dom@19.3.0` | MIT | 2026-10-04T09:19:01.470Z | `未声明` |
| `@vitejs/plugin-react@6.1.2` | MIT | 2026-10-05T10:08:27.439Z | `^20.19.0 || >=22.12.0` |
| `eslint@10.12.0` | MIT | 2026-10-02T20:08:19.639Z | `^20.19.0 || ^22.13.0 || >=24` |
| `eslint-plugin-react-hooks@7.1.1` | MIT | 2026-10-06T16:31:39.917Z | `>=18` |
| `eslint-plugin-react-refresh@0.5.7` | MIT | 2026-09-14T16:12:50.816Z | `未声明` |
| `globals@17.13.0` | MIT | 2026-10-01T03:57:12.091Z | `>=18` |
| `jsdom@29.1.1` | MIT | 2026-10-04T09:15:00.715Z | `^20.19.0 || ^22.13.0 || >=24.0.0` |
| `typescript@6.0.3` | Apache-2.0 | 2026-10-07T08:31:09.380Z | `>=14.17` |
| `typescript-eslint@8.71.1` | MIT | 2026-10-07T01:19:32.394Z | `^18.18.0 || ^20.9.0 || >=21.1.0` |
| `vite@8.3.3` | MIT | 2026-10-06T05:49:07.400Z | `^20.19.0 || >=22.12.0` |
| `vitest@5.0.3` | MIT | 2026-09-30T11:30:42.795Z | `^22.12.0 || ^24.0.0 || >=26.0.0` |
| `react@19.3.0` | MIT | 2026-10-06T16:32:13.254Z | `>=0.10.0` |
| `react-dom@19.3.0` | MIT | 2026-10-06T16:32:34.274Z | `未声明` |
| `react-router@8.4.0` | MIT | 2026-09-15T15:23:42.812Z | `>=22.22.0` |

官方仓库来源：React/React DOM：<https://github.com/react/react>；React Router：<https://github.com/remix-run/react-router>；Vite：<https://github.com/vitejs/vite>；Vite React 插件：<https://github.com/vitejs/vite-plugin-react>；Vitest：<https://github.com/vitest-dev/vitest>；TypeScript：<https://github.com/microsoft/TypeScript>；typescript-eslint：<https://github.com/typescript-eslint/typescript-eslint>；ESLint：<https://github.com/eslint/eslint>；React Hooks lint：<https://github.com/facebook/react>；React Refresh lint：<https://github.com/ArnaudBarre/eslint-plugin-react-refresh>；globals：<https://github.com/sindresorhus/globals>；jsdom：<https://github.com/jsdom/jsdom>；类型包：<https://github.com/DefinitelyTyped/DefinitelyTyped>。

兼容选择：

- TypeScript 最新 registry 版本为 7.0.2，而 typescript-eslint 8.71.1 的 TypeScript peer 范围为 `>=4.8.4 <6.1.0`，因此固定稳定兼容版 6.0.3。
- jsdom 最新 registry 版本为 30.1.2，要求 `^22.22.2 || ^24.15.0 || >=26.0.0`；当前 Node 为 22.22.1，因此固定兼容版 29.1.1。所选组合同时接受文档中 Node 24.11.1。
- React Router 8.4.0 要求 React/React DOM `>=19.2.7`，当前固定 19.3.0 满足；用于按路由 lazy 加载初始化页及未找到页。
- 根 package.json 的所有直接依赖使用精确版本；提交 package-lock.json 固定完整依赖树。只引入 T01 空路由与工程校验需要的依赖，数据请求、表单、UI 基础库在对应实现任务中引入。

## 实际验证

| 命令/操作 | 退出码 | 实际结果 |
|---|---|---|
| `npm install --package-lock-only --ignore-scripts` | 0 | 创建精确锁文件，解析 248 个包 |
| `npm ci` | 0 | 安装 221 个包，安装审计 0 漏洞 |
| `npm audit --json` | 0 | 锁文件漏洞 info/low/moderate/high/critical/total 均为 0 |
| `npm ls --all` | 0 | 已安装依赖无缺失的必需 peer；平台专属及未使用的可选 peer 未安装 |
| `npm run check --workspaces` | 0 | 三应用 TypeScript strict 检查通过 |
| `node node_modules/eslint/bin/eslint.js . --max-warnings 0` | 0 | 根脚本及三应用的 ESLint 严格类型规则通过，0 warning |
| `npm run build --workspaces` | 0 | 三应用 Vite production build 成功，初始化页及未找到页产生独立 lazy chunk |
| `npm run test:unit --workspaces` | 0 | 三应用共 3 个测试文件、6 个路由行为案例通过 |

各应用 Vitest 路由行为验证首页存在一个 `main` 和一个 `h1`，标明本应用 T01 初始化状态；未知路径呈现“页面不存在”且提供返回 `/` 的语义链接。配置不启用 `passWithNoTests`。测试文件创建前实际运行曾返回 1（No test files found），随后新增真实路由行为测试并通过。

Vite 的实测启动结果：

```text
identity-web: port=5173 GET /=200 title=T01 初始化
demo-a: port=5174 GET /=200 title=T01 初始化
demo-b: port=5175 GET /=200 title=T01 初始化
```

三个应用以 Vite `createServer` 真实启动后通过 HTTP fetch 验证上述结果，并关闭服务。另设置 `HOST=0.0.0.0` 后重复验证绑定与首页可达；使用临时本地 HTTP 回显服务设置 `IDENTITY_API_PROXY`，确认 identity-web 的 `/api/t01-proxy-verification` 与 `/health/live` 两个请求均转发原始路径。此项仅验证开发代理配置，没有将临时回显结果视为真实 API/数据库健康验收。

本地默认 host 为 `127.0.0.1`，identity-web 的 `/api` 与 `/health` 默认代理 `http://127.0.0.1:8080`；容器通过 `HOST` 和 `IDENTITY_API_PROXY` 显式覆盖。端口固定为 5173/5174/5175，并启用 strictPort，避免占用时静默切换端口。
