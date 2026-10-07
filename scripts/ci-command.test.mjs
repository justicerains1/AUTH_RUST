import assert from 'node:assert/strict';
import test from 'node:test';
import { sanitizeDiagnostic, runCiCommand } from './ci-command.mjs';

test('CI diagnostics preserve compiler errors while removing credentials and private material', () => {
  const secret = 'CONFIGURATION_SENTINEL_SECRET';
  const token = `eyJ${'a'.repeat(24)}.${'b'.repeat(24)}.${'c'.repeat(24)}`;
  const diagnostic = `error: failed to find OpenSSL\n${secret}\npostgres://user:password@host/database\nBearer short-secret\n${token}\n-----BEGIN PRIVATE KEY-----\nprivate-sentinel\n-----END PRIVATE KEY-----\n${'f'.repeat(80)}`;
  const safe = sanitizeDiagnostic(diagnostic, { DATABASE_PASSWORD: secret });
  assert.match(safe, /error: failed to find OpenSSL/u);
  for (const value of [secret, 'user:password', 'short-secret', token, 'private-sentinel', 'f'.repeat(80)]) {
    assert.ok(!safe.includes(value));
  }
});

test('CI wrapper rejects commands outside its read-only verification list', async () => {
  await assert.rejects(runCiCommand('dev:secrets'), /CI command must/u);
});
