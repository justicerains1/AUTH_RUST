import { access, readdir } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { generateDevSecrets } from './dev-secrets.mjs';
import { migrationArguments, migrationEnvironment } from './database-target.mjs';
import { CommandError, runProcess } from './process.mjs';
import { verifyWorkspace } from './verify-docs.mjs';
import { runSuiteManifest } from './suite-manifest.mjs';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const APPS = ['identity-web', 'demo-a', 'demo-b'];

async function requireFile(path, hint) {
  try { await access(path); } catch { throw new CommandError(hint); }
}

async function nodeTool(relativePath, args, cwd = ROOT) {
  const path = resolve(ROOT, 'node_modules', relativePath);
  await requireFile(path, `缺少 Node 依赖 ${relativePath}；在仓库根目录执行 npm ci 后重试。`);
  return runProcess(process.execPath, [path, ...args], { cwd });
}

function noArguments(command, args) {
  if (args.length) throw new CommandError(`${command} 不接受额外参数。`);
}

async function integration(args) {
  if (args.some((arg) => !/^--task=T\d{2}$/u.test(arg)) || args.length > 1) {
    throw new CommandError('用法：npm run test:integration -- --task=T01（任务参数只能提供一次）。');
  }
  const task = args[0]?.slice('--task='.length);
  if (task && !['T01', 'T03', 'T04', 'T05', 'T06', 'T07', 'T08', 'T09', 'T10', 'T11', 'T12', 'T13', 'T14', 'T17', 'T18', 'T19', 'T20', 'T22'].includes(task)) throw new CommandError(`${task} 的真实集成套件尚未实现；不能作为验收通过。`);
  if (!task) { const code = await runSuiteManifest('integration'); if (code !== 0) throw new CommandError('完整集成套件包含失败，请检查逐模块实际报告。', code); return; }
  const path = resolve(ROOT, 'tests', 'integration', `${task}.mjs`);
  await requireFile(path, `${task} 的真实 Postgres/Redis/API 集成测试尚未创建；不能作为验收通过。`);
  await runProcess(process.execPath, [path], { cwd: ROOT });
}

/** The same entry points are used by local npm commands and CI. */
export async function runCommand(command, args = []) {
  if (command === 'seed:acceptance') {
    noArguments(command, args);
    return runProcess(process.execPath, [resolve(ROOT, 'tests/load/run.mjs'), '--seed-only'], { cwd: ROOT });
  }
  if (command === 'test:load') {
    if (args.length !== 1 || !/^--scenario=(introspection|account|password|mixed)$/u.test(args[0])) throw new CommandError('test:load要求明确 --scenario=introspection|account|password|mixed。');
    return runProcess(process.execPath, [resolve(ROOT, 'tests/load/run.mjs'), args[0]], { cwd: ROOT });
  }
  if (command === 'test:integration') return integration(args);
  if (command === 'test:e2e') {
    if (args.length === 0) { const code = await runSuiteManifest('e2e'); if (code !== 0) throw new CommandError('完整E2E套件包含失败，请检查逐模块实际报告。', code); return; }
    if (args.length === 1 && ['--task=T05', '--task=T06', '--task=T07', '--task=T08', '--task=T09', '--task=T10', '--task=T12', '--task=T13', '--task=T17', '--task=T18', '--task=T19'].includes(args[0])) {
      const task = args[0].slice('--task='.length);
      await runProcess(process.execPath, [resolve(ROOT, `tests/integration/${task}.mjs`), '--e2e'], { cwd: ROOT });
      return;
    }
    throw new CommandError('全量 E2E 尚未完成；已实现注册/邮件验证可执行 npm run test:e2e -- --task=T05。');
  }
  if (command === 'test:security') {
    if (args.length === 1 && ['--task=T04', '--task=T08', '--task=T09', '--task=T11', '--task=T12', '--task=T20'].includes(args[0])) return integration(args);
    if (args.length !== 0) throw new CommandError('安全套件只接受已实现的单次任务选择。');
    const { runSecurityChecks } = await import('./security-checks.mjs');
    const code = await runSecurityChecks(); if (code !== 0) throw new CommandError('安全扫描或协议套件有失败，请检查真实报告。', code); return;
  }
  if (command === 'db:migrate') {
    const selection = migrationArguments(args);
    const env = await migrationEnvironment(ROOT, selection);
    await runProcess('cargo', ['run', '--package', 'identity-store', '--bin', 'identity-migrate', '--locked', '--', ...(selection.allowProduction ? ['--allow-production'] : []), ...(selection.environment === 'test' ? ['--test-target'] : [])], { cwd: ROOT, env });
    return;
  }
  noArguments(command, args);
  switch (command) {
    case 'check':
      await runProcess('cargo', ['fmt', '--all', '--', '--check'], { cwd: ROOT });
      await runProcess('cargo', ['clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings'], { cwd: ROOT });
      for (const app of APPS) await nodeTool('typescript/bin/tsc', ['--noEmit', '--project', resolve(ROOT, 'apps', app, 'tsconfig.json')]);
      await nodeTool('typescript/bin/tsc', ['--noEmit', '--project', resolve(ROOT, 'tests/e2e/tsconfig.json')]);
      await nodeTool('eslint/bin/eslint.js', ['.', '--max-warnings', '0']);
      return;
    case 'build':
      await runProcess('cargo', ['build', '--workspace', '--locked', '--release'], { cwd: ROOT });
      for (const app of APPS) await nodeTool('vite/bin/vite.js', ['build'], resolve(ROOT, 'apps', app));
      return;
    case 'test:unit': {
      const listed = await runProcess('cargo', ['test', '--workspace', '--lib', '--bins', '--locked', '--', '--list'], { cwd: ROOT, capture: true });
      if (!/^.+: test\r?$/mu.test(listed.stdout)) throw new CommandError('Rust 单元测试尚未创建；不能把零测试作为验收通过。');
      await runProcess('cargo', ['test', '--workspace', '--lib', '--bins', '--locked'], { cwd: ROOT });
      for (const app of APPS) await nodeTool('vitest/vitest.mjs', ['run'], resolve(ROOT, 'apps', app));
      return;
    }
    case 'test:tooling': {
      const tests = (await readdir(resolve(ROOT, 'scripts'))).filter((path) => path.endsWith('.test.mjs')).sort();
      if (!tests.length) throw new CommandError('脚本行为测试尚未创建；不能把零测试作为验收通过。');
      await runProcess(process.execPath, ['--test', ...tests.map((path) => resolve(ROOT, 'scripts', path))], { cwd: ROOT });
      return;
    }
    case 'verify:docs': {
      const report = verifyWorkspace(ROOT);
      console.log(`文档检查通过：${report.tasks} 个任务、${report.steps} 个步骤映射、${report.cases} 个案例。`);
      return;
    }
    case 'verify:openapi': {
      const { verifyOpenApiWorkspace } = await import('./verify-openapi.mjs');
      const report = await verifyOpenApiWorkspace(ROOT);
      console.log(`OpenAPI 检查通过：${report.operations} 个操作（${report.jsonOperations} 个 JSON API）、${report.schemas} 个 schema、${report.examples} 个 examples；契约验证不代表功能已实现。`);
      return;
    }
    case 'test:accessibility': {
      const suite = resolve(ROOT, 'tests', 'accessibility', 'T16.mjs');
      await requireFile(suite, '缺少 T16 浏览器可访问性套件；不能作为验收通过。');
      await runProcess(process.execPath, [suite], { cwd: ROOT });
      const product = resolve(ROOT, 'tests', 'accessibility', 'product.mjs');
      await requireFile(product, '缺少产品页面真实浏览器可访问性套件；不能作为验收通过。');
      await runProcess(process.execPath, [product], { cwd: ROOT });
      return;
    }
    case 'dev:secrets':
      return generateDevSecrets(ROOT);
    case 'dev:up':
    case 'dev:down': {
      await requireFile(resolve(ROOT, '.local', 'dev.env'), '缺少 .local/dev.env；先执行 npm run dev:secrets 生成本地秘密。');
      await runProcess('docker', ['compose', '--env-file', resolve(ROOT, '.local', 'dev.env'), '-f', resolve(ROOT, 'infra', 'compose.dev.yaml'), ...(command === 'dev:up' ? ['up', '-d', '--wait', '--build'] : ['down'])], { cwd: ROOT });
      return;
    }
    default:
      throw new CommandError(`未知根命令：${command ?? '(未指定)'}。请使用 package.json 中定义的脚本。`);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    await runCommand(process.argv[2], process.argv.slice(3));
  } catch (error) {
    console.error(error.message);
    process.exitCode = error.exitCode ?? 1;
  }
}
