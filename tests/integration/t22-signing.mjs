// Explicit local-only T22 signing drill. Root T22 runner integration is coordinated separately.
import assert from 'node:assert/strict';
import { createHash, randomBytes } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { localEnvironment, validateDatabaseTarget } from '../../scripts/database-target.mjs';
import { runStage } from '../../scripts/full-test.mjs';

const root = resolve(import.meta.dirname, '../..');
const privateDirectory = resolve(root, `.local/t22-signing-${randomBytes(8).toString('hex')}`);
const evidence = resolve(root, 'docs/evidence/T22');
const started = new Date().toISOString();
const report = { started, kind: 'Local real HTTP/PG/BFF signing rollout; not production distribution or elapsed 12-hour observation', results: [], sourceSha256: {}, exitCode: 1 };
try {
  const local = await localEnvironment(root);
  const supplied = process.env.TEST_DATABASE_URL ?? process.env.DATABASE_URL;
  const environment = process.env.APP_ENV ?? 'test';
  if (supplied) validateDatabaseTarget(environment, supplied, { testOnly: true });
  else assert.equal(environment, 'test');
  const database = new URL(supplied ?? local.DATABASE_URL);
  if (!supplied) { database.hostname = '127.0.0.1'; database.pathname = '/identity_test'; database.search = ''; }
  validateDatabaseTarget('test', database.toString(), { testOnly: true });
  const redis = new URL(local.REDIS_URL); redis.hostname = '127.0.0.1';
  await mkdir(privateDirectory, { recursive: true, mode: 0o700 });
  const oldKey = resolve(privateDirectory, 'old.pem'); const newKey = resolve(privateDirectory, 'new.pem');
  // Real fresh RSA key pairs in this test-only directory; never mutate existing signing files.
  for (const file of [oldKey, newKey]) execFileSync('openssl', ['genpkey', '-algorithm', 'RSA', '-pkeyopt', 'rsa_keygen_bits:2048', '-out', file], { stdio: ['ignore', 'ignore', 'pipe'] });
  const keyMap = resolve(privateDirectory, 'encryption-keys.json');
  const active = `signing-${randomBytes(8).toString('hex')}`;
  const aead = randomBytes(32).toString('base64');
  await writeFile(keyMap, JSON.stringify({ [active]: aead }), { mode: 0o600 });
  const password = `Local signing drill ${randomBytes(32).toString('base64url')}`;
  const secrets = [password, aead, database.toString(), redis.toString()];
  const env = { ...process.env, APP_ENV: 'test', TEST_DATABASE_URL: database.toString(), DATABASE_URL: database.toString(), REDIS_URL: redis.toString(),
    T22_SIGNING_DIRECTORY: privateDirectory, T22_SIGNING_OLD_FILE: oldKey, T22_SIGNING_NEW_FILE: newKey,
    T22_SIGNING_ENCRYPTION_KEYS_FILE: keyMap, T22_SIGNING_ACTIVE_ENCRYPTION_KID: active, T22_SIGNING_PASSWORD: password };
  const result = await runStage(process.env.T22_SIGNING_CARGO ?? 'cargo', ['test', '-p', 'identity-server', '--test', 't22_signing', '--locked', '--', '--exact', 't22_real_signing_rollout', '--nocapture'], { cwd: root, env });
  await writeFile(resolve(privateDirectory, 'raw-test.txt'), result.diagnostic, { mode: 0o600 });
  const passes = result.diagnostic.split('\n').filter((line) => line.startsWith('PASS T22'));
  report.results = passes;
  report.rustExitCode = result.exitCode;
  report.zeroIgnored = /1 passed; 0 failed; 0 ignored/u.test(result.diagnostic);
  for (const file of ['crates/identity-server/tests/t22_signing.rs', 'tests/integration/t22-signing.mjs', 'crates/identity-core/src/jose.rs', 'crates/demo-bff/src/oidc.rs', 'Cargo.lock']) {
    report.sourceSha256[file] = createHash('sha256').update(await readFile(resolve(root, file))).digest('hex');
  }
  report.commit = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim();
  if (result.exitCode !== 0) {
    let safe = result.diagnostic;
    for (const secret of secrets) safe = safe.replaceAll(secret, '[REDACTED]');
    safe = safe.replace(/\b[A-Za-z0-9_-]{43,}\b/gu, '[OPAQUE]');
    await mkdir(evidence, { recursive: true });
    await writeFile(resolve(evidence, `signing-rotation-failure-${Date.now()}.txt`), safe);
  }
  assert.equal(result.exitCode, 0, 'Local signing rollout failed; private diagnostics were suppressed and a sanitized report was retained.');
  assert.ok(report.zeroIgnored, 'The real signing integration must execute, never skip.');
  assert.equal(passes.length, 7, 'All required signing rollout, negative removal, rollback and boundary checks must execute.');
  report.exitCode = 0;
  console.log('T22 local signing rollout: real HTTP/PG/BFF key publication, switching, cache refresh, rollback and precise hint/session boundaries passed.');
} catch (error) {
  report.failure = error.message;
  process.exitCode = 1;
  console.error(error.message);
} finally {
  report.completed = new Date().toISOString();
  await mkdir(evidence, { recursive: true });
  await writeFile(resolve(evidence, `signing-rotation-${report.exitCode === 0 ? 'passed' : 'failure'}-${Date.now()}.json`), JSON.stringify(report, null, 2) + '\n');
  await rm(privateDirectory, { recursive: true, force: true });
}
