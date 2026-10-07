import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdir, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { cpus, totalmem, platform, release } from 'node:os';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const ROOT_COMMANDS = ['verify:docs', 'verify:openapi', 'test:tooling', 'check', 'test:unit', 'test:integration', 'test:e2e', 'test:security', 'test:accessibility', 'build'];

function npmCli() {
  const candidates = [process.env.npm_execpath, resolve(dirname(process.execPath), 'node_modules/npm/bin/npm-cli.js'), '/usr/share/nodejs/npm/bin/npm-cli.js'];
  return candidates.find((path) => typeof path === 'string' && existsSync(path));
}

/** Spawn without a shell; diagnostics stay private so failed tests cannot publish credentials. */
export function runStage(program, args, { cwd = ROOT, env = process.env } = {}) {
  return new Promise((resolveResult) => {
    const child = spawn(program, args, { cwd, env, shell: false, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
    let diagnostic = '';
    child.stdout.setEncoding('utf8'); child.stderr.setEncoding('utf8');
    const collect = (chunk) => { if (diagnostic.length < 8 * 1024 * 1024) diagnostic += chunk; };
    child.stdout.on('data', collect); child.stderr.on('data', collect);
    child.on('error', (error) => resolveResult({ exitCode: 1, diagnostic: `Unable to start tool (${error.code ?? 'unknown'}).`, signal: null }));
    child.on('close', (code, signal) => resolveResult({ exitCode: code ?? 1, diagnostic, signal }));
  });
}

/** Continue after a failing stage and preserve every exit status; never turn missing suites green. */
export async function executeStages(stages, executor = runStage) {
  const results = [];
  for (const stage of stages) {
    const started = new Date().toISOString(); const instant = Date.now();
    let result;
    try { result = await executor(stage.program, stage.args); }
    catch { result = { exitCode: 1, diagnostic: 'Stage execution failed; private diagnostics unavailable.', signal: null }; }
    results.push({ name: stage.name, started, durationMs: Date.now() - instant, ...result });
  }
  return results;
}

export function summaryMarkdown(results, environment, privateDirectory) {
  const failed = results.filter((result) => result.exitCode !== 0);
  return `# 完整测试总结\n\n生成时间：${new Date().toISOString()}。\n代码版本：${environment.commit}；工作区：${environment.dirty ? '包含未提交变更' : '干净'}。\n环境：${environment.os}；Node ${environment.node}；${environment.cpuCount} logical CPU；内存 ${environment.memoryGiB} GiB。\n\n本报告记录全部命令的实际执行；单项失败后仍运行后续检查。测试产物的原始输出保存在本机受限目录，不提交含凭据的原始诊断。\n\n| 检查 | 实际退出码 | 耗时（秒） | 结果 |\n|---|---:|---:|---|\n${results.map((result) => `| ${result.name} | ${result.exitCode} | ${(result.durationMs / 1000).toFixed(2)} | ${result.exitCode === 0 ? '通过' : '失败'} |`).join('\n')}\n\n## 结论\n\n${failed.length ? `未通过：${failed.map((result) => result.name).join('、')}。完整验收未放行，不能将局部通过视为完整系统通过。` : '以上全套自动检查通过；真实生产、设备、性能、恢复等人工或独立环境验收仍须核对 acceptance.md，不由此报告自动放行。'}\n\n本机原始诊断：${privateDirectory}。详细任务失败原因与后续状态见 acceptance.md 和各任务证据。本报告不修改任务状态。用户要求测试失败时也提交总结，代码推送不代表生产发布。\n`;
}

export async function runFullTest() {
  const stamp = new Date().toISOString().replace(/[:.]/gu, '-');
  const privateDirectory = resolve(ROOT, '.local', 'full-test', stamp);
  await mkdir(privateDirectory, { recursive: true, mode: 0o700 });
  const revision = await runStage('git', ['rev-parse', 'HEAD']);
  const status = await runStage('git', ['status', '--porcelain=v1']);
  const commit = revision.exitCode === 0 && /^[a-f0-9]{40}$/u.test(revision.diagnostic.trim()) ? revision.diagnostic.trim() : 'unavailable';
  const environment = { commit, dirty: status.exitCode !== 0 || status.diagnostic.length > 0, os: `${platform()} ${release()}`, node: process.version, cpuCount: cpus().length, memoryGiB: (totalmem() / 1024 ** 3).toFixed(2) };
  const cli = npmCli();
  const stages = [
    { name: 'npm ci', program: process.execPath, args: cli ? [cli, 'ci'] : ['-e', 'process.exit(1)'] },
    ...ROOT_COMMANDS.map((name) => ({ name, program: process.execPath, args: [resolve(ROOT, 'scripts/run.mjs'), name] })),
  ];
  const results = await executeStages(stages, async (program, args) => {
    console.log(`正在运行完整检查 ${stages.find((stage) => stage.args === args)?.name ?? program}…`);
    const result = await runStage(program, args);
    console.log(`检查退出码：${result.exitCode}`);
    return result;
  });
  for (const [index, result] of results.entries()) {
    await writeFile(resolve(privateDirectory, `${String(index + 1).padStart(2, '0')}.txt`), result.diagnostic, { mode: 0o600 });
  }
  const report = summaryMarkdown(results, environment, `.local/full-test/${stamp}/`);
  const evidence = resolve(ROOT, 'docs/evidence/full-test'); await mkdir(evidence, { recursive: true });
  await writeFile(resolve(evidence, `${stamp}.md`), report);
  await writeFile(resolve(ROOT, 'TEST_SUMMARY.md'), report);
  console.log('完整检查结果已写入 TEST_SUMMARY.md；原始输出仅保存在 .local/full-test/。');
  return results.some((result) => result.exitCode !== 0) ? 1 : 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  process.exitCode = await runFullTest();
}
