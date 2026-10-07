import assert from 'node:assert/strict';
import test from 'node:test';
import { executeStages, summaryMarkdown, runStage } from './full-test.mjs';

test('full test keeps later stages after a failure and records missing commands as failures', async () => {
  const stages = [{ name: 'first', program: 'fixture', args: ['first'] }, { name: 'second', program: 'fixture', args: ['second'] }, { name: 'third', program: 'fixture', args: ['third'] }];
  const visited = [];
  const results = await executeStages(stages, async (_program, args) => {
    visited.push(args[0]);
    return { exitCode: args[0] === 'second' ? 3 : 0, diagnostic: '', signal: null };
  });
  assert.deepEqual(visited, ['first', 'second', 'third']);
  assert.deepEqual(results.map((result) => result.exitCode), [0, 3, 0]);
});

test('full summary reports failure without persisting private process output', () => {
  const results = [{ name: 'integration', started: '', durationMs: 100, exitCode: 1, diagnostic: 'PASSWORD_SENTINEL', signal: null }];
  const environment = { commit: 'fixture', dirty: true, os: 'fixture', node: 'fixture', cpuCount: 1, memoryGiB: '1' };
  const summary = summaryMarkdown(results, environment, '.local/full-test/fixture');
  assert.match(summary, /未通过：integration/u);
  assert.doesNotMatch(summary, /PASSWORD_SENTINEL/u);
  assert.match(summary, /包含未提交变更/u);
});

test('missing executable has a nonzero result without interrupting report generation', async () => {
  const result = await runStage('identity-missing-full-test-executable', []);
  assert.notEqual(result.exitCode, 0);
});
