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

test('load tests require an explicit valid scenario without spawning a process', async () => {
  await assert.rejects(runCommand('test:load'), /要求明确/u);
  await assert.rejects(runCommand('test:load', ['--scenario=production']), /要求明确/u);
});
test('seed refuses extra target or production selectors before starting', async () => {
  await assert.rejects(runCommand('seed:acceptance', ['--env=production']), /不接受额外参数/u);
});

test('unsupported complete suite selectors fail without running a child', async () => {
  await assert.rejects(runCommand('test:security', ['--task=T23']), /只接受已实现/u);
  await assert.rejects(runCommand('test:e2e', ['--task=T23']), /全量 E2E 尚未完成/u);
});

test('integration selection rejects missing tasks and invalid or duplicate selectors', async () => {
  await assert.rejects(runCommand('test:integration', ['--task=T02']), /尚未实现/u);
  await assert.rejects(runCommand('test:integration', ['--task=T01', '--task=T01']), /只能提供一次/u);
  await assert.rejects(runCommand('test:integration', ['--task=../../something']), /用法/u);
});
