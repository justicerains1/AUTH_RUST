import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import test from 'node:test';
import { alertFixtures } from './alert-fixtures.mjs';

const root = resolve(import.meta.dirname, '../..');
test('real promtool verifies host alert thresholds, holds, missing telemetry, stale files and recovery', (t) => {
  const fixture = mkdtempSync(join(tmpdir(), 'identity-alert-rules-')); t.after(() => rmSync(fixture, { recursive: true, force: true }));
  const promtool = process.env.PROMTOOL_BINARY ?? join(root, '.local/security-tools/prometheus-3.15.0.linux-amd64/promtool');
  assert.match(execFileSync(promtool, ['--version'], { encoding: 'utf8' }), /3\.15\.0/u);
  const rules = join(root, 'infra/ops/identity-alerts.yaml'); const input = join(fixture, 'unit-test.json'); const fixtures = alertFixtures(rules); writeFileSync(input, JSON.stringify(fixtures));
  assert.equal(fixtures.tests.length, 20); assert.match(execFileSync(promtool, ['check', 'rules', rules], { encoding: 'utf8' }), /18 rules found/u);
  assert.match(execFileSync(promtool, ['test', 'rules', input], { encoding: 'utf8', maxBuffer: 1024 * 1024 }), /SUCCESS/u);
});
