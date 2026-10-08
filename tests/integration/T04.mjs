// Tests security primitives using real PostgreSQL/Redis and a temporary Axum fixture.
// The fixture never implements registration/login or declares authenticated success.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdir, rm, writeFile } from 'node:fs/promises';
import { randomBytes } from 'node:crypto';
import { resolve } from 'node:path';
import { localEnvironment, validateDatabaseTarget } from '../../scripts/database-target.mjs';

const root = resolve(import.meta.dirname, '../..');
const evidence = resolve(root, 'docs/evidence/T04');
const privateDirectory = resolve(root, '.local', `t04-${randomBytes(8).toString('hex')}`);
const composePrefix = ['compose', '--env-file', resolve(root, '.local/dev.env'), '-f', resolve(root, 'infra/compose.dev.yaml')];
const lines = [`T04 security primitives; real dependencies; ${new Date().toISOString()}`];

function childResult(program, args, env = process.env) {
  return new Promise((resolveResult, reject) => {
    const child = spawn(program, args, { cwd: root, env, shell: false, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = ''; let stderr = '';
    child.stdout.setEncoding('utf8'); child.stderr.setEncoding('utf8');
    child.stdout.on('data', (data) => { stdout += data; }); child.stderr.on('data', (data) => { stderr += data; });
    child.on('error', () => reject(new Error(`${program} unavailable; install the documented tool.`)));
    child.on('close', (code) => resolveResult({ code, stdout, stderr }));
  });
}

async function compose(...args) {
  const result = await childResult('docker', [...composePrefix, ...args]);
  assert.equal(result.code, 0, `Compose ${args[0]} failed; child diagnostics suppressed.`);
  return result;
}

async function main() {
  const supplied = process.env.TEST_DATABASE_URL ?? process.env.DATABASE_URL;
  const appEnv = process.env.APP_ENV ?? 'test';
  if (supplied) validateDatabaseTarget(appEnv, supplied, { testOnly: true });
  else assert.equal(appEnv, 'test', 'T04 requires test environment before connecting');
  const local = await localEnvironment(root);
  const url = new URL(supplied ?? local.DATABASE_URL);
  if (!supplied) { url.hostname = '127.0.0.1'; url.pathname = '/identity_test'; url.search = ''; }
  validateDatabaseTarget('test', url.toString(), { testOnly: true });
  await compose('exec', '-T', 'redis', 'redis-cli', 'PING');
  const database = await compose('exec', '-T', 'postgres', 'psql', '-U', 'identity', '-d', 'postgres', '-Atc', "SELECT datname FROM pg_database WHERE datname='identity_test'");
  if (!database.stdout.trim()) await compose('exec', '-T', 'postgres', 'createdb', '-U', 'identity', 'identity_test');
  await mkdir(privateDirectory, { recursive: true, mode: 0o700 });
  const encryptionPath = resolve(privateDirectory, 'encryption-keys.json');
  const encryptionKey = randomBytes(32).toString('base64');
  await writeFile(encryptionPath, `${JSON.stringify({ 't04-fixture': encryptionKey })}\n`, { mode: 0o600 });
  const smtpPasswordPath = resolve(privateDirectory, 'smtp-password');
  await writeFile(smtpPasswordPath, randomBytes(32).toString('hex'), { mode: 0o600 });
  const redisUrl = new URL(local.REDIS_URL); redisUrl.hostname = '127.0.0.1';
  const env = { ...process.env, APP_ENV: 'test', TEST_DATABASE_URL: url.toString(), DATABASE_URL: url.toString(), REDIS_URL: redisUrl.toString(),
    T04_SIGNING_KEY_FILE: resolve(root, '.local/signing.pem'), T04_ENCRYPTION_KEYS_FILE: encryptionPath, T04_ACTIVE_ENCRYPTION_KID: 't04-fixture', T04_SMTP_PASSWORD_FILE: smtpPasswordPath };
  const secretValues = [url.password, url.toString(), encryptionKey];
  const testCase = async (name, mode) => {
    const result = await childResult('cargo', ['test', '--package', 'identity-server', '--test', 't04_security', '--locked', '--', '--exact', name, '--nocapture'], { ...env, T04_DEPENDENCY_MODE: mode });
    const output = result.stdout + result.stderr;
    for (const value of secretValues) assert.ok(!output.includes(value), 'Security suite output leaked private configuration');
    if (result.code !== 0) { const safe = output.replaceAll(url.toString(), '[DATABASE]').replaceAll(encryptionKey, '[KEY]').replace(/\b[0-9a-f]{8}-[0-9a-f-]{27}\b/giu, '[UUID]').replace(/[A-Za-z0-9+/_=-]{64,}/gu, '[LONG VALUE]'); await mkdir(evidence, { recursive: true }); await writeFile(resolve(evidence, `redis-connection-diagnostic-${Date.now()}.txt`), safe); }
    assert.equal(result.code, 0, `${name} failed; child output suppressed to protect secrets.`);
    assert.match(result.stdout, /1 passed/u, `${name} did not execute a real test`);
    for (const line of result.stdout.split('\n')) if (line.startsWith('PASS T04')) lines.push(line);
  };
  await testCase('t04_security_boundary', 'healthy');
  await testCase('t04_redis_connection_recovery', 'healthy');
  await compose('stop', 'redis');
  try { await testCase('t04_redis_unavailable', 'redis-stopped'); }
  finally { await compose('start', 'redis'); }
  // Explicitly acknowledge dependency recovery; no authentication expiry sleeps.
  await compose('up', '-d', '--wait', 'redis');
  await compose('stop', 'postgres');
  try { await testCase('t04_postgres_unavailable', 'postgres-stopped'); }
  finally { await compose('start', 'postgres'); }
  await compose('up', '-d', '--wait', 'postgres');
  console.log('T04 real security middleware, atomic budgets, CSRF, proxy rejection and dependency failures passed (temporary fixture only).');
}

let passed = false;
try { await main(); passed = true; }
catch (error) { lines.push(`FAIL ${error.message}`); console.error(error.message); process.exitCode = 1; }
finally {
  lines.push(`Completed ${new Date().toISOString()}`, `Exit code ${passed ? 0 : 1}`);
  await mkdir(evidence, { recursive: true });
  await writeFile(resolve(evidence, passed ? 'integration.txt' : 'integration-failure.txt'), `${lines.join('\n')}\n`);
  await rm(privateDirectory, { recursive: true, force: true });
}
