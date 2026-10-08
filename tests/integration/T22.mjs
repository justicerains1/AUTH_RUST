// Local metrics, keys, encrypted backup boundaries and production configuration; deployment remains manual.
import { resolve } from 'node:path';
import { mkdir, writeFile } from 'node:fs/promises';
import { localEnvironment, validateDatabaseTarget } from '../../scripts/database-target.mjs';
import { runStage } from '../../scripts/full-test.mjs';
const root = resolve(import.meta.dirname, '../..');
const local = await localEnvironment(root);
const url = new URL(process.env.TEST_DATABASE_URL ?? local.DATABASE_URL);
if (!process.env.TEST_DATABASE_URL) { url.hostname = '127.0.0.1'; url.pathname = '/identity_test'; url.search = ''; }
validateDatabaseTarget(process.env.APP_ENV ?? 'test', url.toString(), { testOnly: true });
const redis = new URL(local.REDIS_URL); redis.hostname = '127.0.0.1';
const env = { ...process.env, APP_ENV: 'test', DATABASE_URL: url.toString(), TEST_DATABASE_URL: url.toString(), KEY_DRILL_DATABASE_URL: url.toString(), KEY_DRILL_SIGNING_KEY_FILE: resolve(root, '.local/signing.pem'), REDIS_URL: redis.toString(), T22_METRICS_SIGNING_KEY_FILE: resolve(root, '.local/signing.pem'), T22_METRICS_ENCRYPTION_KEYS_FILE: resolve(root, '.local/encryption-keys.json'), T22_METRICS_ACTIVE_ENCRYPTION_KID: local.ACTIVE_ENCRYPTION_KID };
const privateDirectory = resolve(root, '.local/t22-integration'); await mkdir(privateDirectory, { recursive: true, mode: 0o700 });
const results = [];
for (const [name, packageName, suite] of [['metrics', 'identity-server', 't22_metrics'], ['key-reencryption', 'identity-admin-cli', 't22_keys']]) {
  const result = await runStage('cargo', ['test', '-p', packageName, '--test', suite, '--locked', '--', '--nocapture'], { cwd: root, env });
  await writeFile(resolve(privateDirectory, `${name}.txt`), result.diagnostic, { mode: 0o600 });
  results.push({ name, exitCode: result.exitCode });
  if (result.exitCode === 0 && !/1 passed/u.test(result.diagnostic)) results.at(-1).exitCode = 1;
}
const opsEnvironment = { ...env, AGE_BINARY: process.env.AGE_BINARY ?? resolve(root, '.local/security-tools/age/age'), AGE_KEYGEN_BINARY: process.env.AGE_KEYGEN_BINARY ?? resolve(root, '.local/security-tools/age/age-keygen') };
const ops = await runStage(process.execPath, ['--test', ...['base-backup', 'wal-archive', 'production-config'].map((name) => resolve(root, `tests/ops/${name}.test.mjs`))], { cwd: root, env: opsEnvironment });
await writeFile(resolve(privateDirectory, 'backup-wal-production-config.txt'), ops.diagnostic, { mode: 0o600 });
const opsPassed = ops.exitCode === 0 && /# tests 12\r?\n/u.test(ops.diagnostic) && /# pass 12\r?\n/u.test(ops.diagnostic) && /# skipped 0\r?\n/u.test(ops.diagnostic);
results.push({ name: 'backup-wal-production-config', exitCode: opsPassed ? 0 : ops.exitCode || 1 });
const signing = await runStage(process.execPath, [resolve(root, 'tests/integration/t22-signing.mjs')], { cwd: root, env });
await writeFile(resolve(privateDirectory, 'signing-rotation.txt'), signing.diagnostic, { mode: 0o600 });
results.push({ name: 'signing-rotation', exitCode: signing.exitCode });
const evidence = resolve(root, 'docs/evidence/T22'); await mkdir(evidence, { recursive: true });
const code = results.some((result) => result.exitCode !== 0) ? 1 : 0;
await writeFile(resolve(evidence, 'integration.json'), JSON.stringify({ completed: new Date().toISOString(), results, exitCode: code, scope: 'Local metrics, PostgreSQL key maintenance, isolated encrypted backup/WAL, signing rollout with real BFF and offline production configuration; not production acceptance.' }, null, 2)+'\n');
process.exitCode = code;
