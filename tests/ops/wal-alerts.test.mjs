import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import test from 'node:test';
import { walAlertFixtures } from './wal-alert-fixtures.mjs';
test('real promtool verifies WAL failure, idle observation, missing/future time and publication duration', t => { const root = resolve(import.meta.dirname, '../..'); const directory = mkdtempSync(join(tmpdir(), 'identity-wal-alert-')); t.after(() => rmSync(directory, { recursive: true, force: true })); const tool = process.env.PROMTOOL_BINARY ?? join(root, '.local/security-tools/prometheus-3.15.0.linux-amd64/promtool'); const input = join(directory, 'fixtures.json'); const value = walAlertFixtures(join(root, 'infra/ops/identity-alerts.yaml')); assert.equal(value.tests.length, 11); writeFileSync(input, JSON.stringify(value)); assert.match(execFileSync(tool, ['test', 'rules', input], { encoding: 'utf8' }), /SUCCESS/u); });
