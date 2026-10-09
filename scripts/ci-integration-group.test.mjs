import assert from 'node:assert/strict';
import test from 'node:test';
import { groups, runGroup } from './ci-integration-group.mjs';

test('CI partition preserves every previous integration/browser command exactly once', () => {
  const expected = [
    ['test:integration', 'T01'], ['test:integration', 'T03'], ['test:security', 'T04'],
    ...['T05', 'T06', 'T07', 'T08', 'T09', 'T10', 'T12'].flatMap((task) => [['test:integration', task], ['test:e2e', task]]),
    ...['T11', 'T13', 'T14'].map((task) => ['test:integration', task]),
    ...['T17', 'T18', 'T19'].map((task) => ['test:e2e', task]), ['test:accessibility', ''],
  ].map(([command, task]) => `${command}:${task}`).sort();
  const actual = Object.values(groups).flat().map(([command, task]) => `${command}:${task?.slice(7) ?? ''}`).sort();
  assert.deepEqual(actual, expected); assert.equal(new Set(actual).size, actual.length);
});
test('each group remains sequential and stops on real failure', async () => {
  const visited = []; const code = await runGroup('product', async (_program, args) => { visited.push(args.at(-1)); return { exitCode: visited.length === 2 ? 7 : 0, diagnostic: 'controlled failure' }; });
  assert.equal(code, 7); assert.deepEqual(visited, ['--task=T17', '--task=T18']);
});
test('unknown group fails without running any process', async () => {
  let called = false; await assert.rejects(runGroup('missing', async () => { called = true; }), /Unknown/u); assert.equal(called, false);
});
