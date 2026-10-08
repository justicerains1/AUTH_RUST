import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

test('capacity staircase retains separate fixed measurement windows and rejects limited traffic', async () => {
  const source = (await readFile(new URL('./capacity.js', import.meta.url), 'utf8')).replace(/^import .*;\n/gmu, '').replaceAll('export ', '');
  let now = 0;
  const samples = {};
  class Metric { constructor(name) { this.name = name; samples[name] = []; } add(value) { samples[this.name].push(value); } }
  const context = vm.createContext({ __ENV: { APP_ENV: 'test' }, open: () => JSON.stringify({ base: 'http://127.0.0.1:5310', control: 'http://127.0.0.1:5311', clients: Array.from({ length: 20 }, () => ({})), sessions: Array.from({ length: 2048 }, () => ({})) }), Counter: Metric, Rate: Metric, Trend: Metric, Date: { now: () => now }, execution: { scenario: { startTime: 0, name: 'password_8' } } });
  vm.runInContext(source, context);
  for (const [instant, expected] of [[59_999, false], [60_000, true], [119_999, true], [120_000, false]]) { now = instant; assert.equal(vm.runInContext('measured()', context), expected); }
  assert.equal(vm.runInContext('scenarios.password_8.rate', context), 40);
  assert.equal(vm.runInContext('scenarios.introspection_8.startTime', context), '360s');
  assert.equal(vm.runInContext("thresholds['capacity_requests{endpoint:introspection,multiplier:8}'][0]", context), 'count>=144000');
  vm.runInContext("record('password',{status:429},10,false,true)", context);
  assert.deepEqual(samples.capacity_unexpected_errors, [true]);
  assert.deepEqual(samples.capacity_rate_limited, [1]);
});
