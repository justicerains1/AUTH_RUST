import assert from 'node:assert/strict';
import test from 'node:test';
import { CommandError, runProcess } from './process.mjs';
import { runCommand } from './run.mjs';

test('subprocess errors retain a nonzero exit code', async () => {
  await assert.rejects(runProcess(process.execPath, ['-e', 'process.exit(7)'], { quiet: true }),
    (error) => error instanceof CommandError && error.exitCode === 7);
});

test('missing executables fail with an installation hint', async () => {
  await assert.rejects(runProcess('identity-missing-executable-for-test', []), /缺少.*确认该工具已安装/u);
});

test('shell metacharacters remain literal arguments', async () => {
  const input = 'literal; $(echo forbidden) & `echo forbidden`';
  const result = await runProcess(process.execPath, ['-e', 'process.stdout.write(process.argv[1])', input], { capture: true });
  assert.equal(result.stdout, input);
});

for (const command of ['test:load', 'seed:acceptance']) {
  test(`${command} cannot report success before implementation`, async () => {
    await assert.rejects(runCommand(command), (error) => error instanceof CommandError && error.exitCode !== 0 && /尚未实现/u.test(error.message));
  });
}

test('whole security suite stays nonzero until all protocol tasks are implemented', async () => {
  await assert.rejects(runCommand('test:security'), /全量安全\/协议验收尚未完成/u);
  await assert.rejects(runCommand('test:security', ['--task=T20']), /全量安全\/协议验收尚未完成/u);
});

test('whole E2E suite stays nonzero until remaining tasks are implemented', async () => {
  await assert.rejects(runCommand('test:e2e'), /全量 E2E 尚未完成/u);
  await assert.rejects(runCommand('test:e2e', ['--task=T23']), /全量 E2E 尚未完成/u);
});

test('integration selection rejects missing tasks and invalid or duplicate selectors', async () => {
  await assert.rejects(runCommand('test:integration', ['--task=T02']), /尚未实现/u);
  await assert.rejects(runCommand('test:integration'), /全量集成套件尚未实现/u);
  await assert.rejects(runCommand('test:integration', ['--task=T01', '--task=T01']), /只能提供一次/u);
  await assert.rejects(runCommand('test:integration', ['--task=../../something']), /用法/u);
});
