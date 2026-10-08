// Explicit test-only database plus real authorization API. No trace or secret output.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdir, rm, writeFile } from 'node:fs/promises';
import { randomBytes } from 'node:crypto';
import { resolve } from 'node:path';
import { localEnvironment, validateDatabaseTarget } from '../../scripts/database-target.mjs';

const e2e = process.argv.includes('--e2e');
const root = resolve(import.meta.dirname, '../..');

const privateDirectory = resolve(root, '.local', `t12-${randomBytes(8).toString('hex')}`);
const evidence = resolve(root, 'docs/evidence/T12');
const lines = [`T12 real OIDC integration ${new Date().toISOString()}`];
const composeArgs = ['compose', '--env-file', resolve(root, '.local/dev.env'), '-f', resolve(root, 'infra/compose.dev.yaml')];
function spawnCapture(program, args, env = process.env, input = 'ignore') {
  const child = spawn(program, args, { cwd: root, env, shell: false, windowsHide: true, stdio: [input, 'pipe', 'pipe'] });
  let stdout = ''; let stderr = '';
  child.stdout.setEncoding('utf8'); child.stderr.setEncoding('utf8');
  child.stdout.on('data', (data) => { stdout += data; }); child.stderr.on('data', (data) => { stderr += data; });
  const completion = new Promise((resolveResult, reject) => {
    child.on('error', () => reject(new Error(`${program} unavailable; install documented dependency.`)));
    child.on('close', (code) => resolveResult({ code, stdout, stderr }));
  });
  return { child, completion, output: () => stdout + stderr };
}
async function command(program, args, env) { return spawnCapture(program, args, env).completion; }
async function compose(...args) {
  const result = await command('docker', [...composeArgs, ...args]); assert.equal(result.code, 0, `Compose ${args[0]} failed (diagnostics suppressed).`); return result;
}
async function prepare() {
  const appEnv = process.env.APP_ENV ?? 'test'; const supplied = process.env.TEST_DATABASE_URL ?? process.env.DATABASE_URL;
  if (supplied) validateDatabaseTarget(appEnv, supplied, { testOnly: true }); else assert.equal(appEnv, 'test');
  const local = await localEnvironment(root); const url = new URL(supplied ?? local.DATABASE_URL);
  if (!supplied) { url.hostname = '127.0.0.1'; url.pathname = '/identity_test'; url.search = ''; }
  validateDatabaseTarget('test', url.toString(), { testOnly: true });
  await compose('exec', '-T', 'redis', 'redis-cli', 'PING');
  const database = await compose('exec', '-T', 'postgres', 'psql', '-U', 'identity', '-d', 'postgres', '-Atc', "SELECT datname FROM pg_database WHERE datname='identity_test'");
  if (!database.stdout.trim()) await compose('exec', '-T', 'postgres', 'createdb', '-U', 'identity', 'identity_test');
  await mkdir(privateDirectory, { recursive: true, mode: 0o700 });
  const key = randomBytes(32).toString('base64'); const keyPath = resolve(privateDirectory, 'keys.json');
  await writeFile(keyPath, JSON.stringify({ 't12-fixture': key }), { mode: 0o600 });
  const smtpPasswordPath = resolve(privateDirectory, 'smtp-password');
  await writeFile(smtpPasswordPath, randomBytes(32).toString('hex'), { mode: 0o600 });
  const redis = new URL(local.REDIS_URL); redis.hostname = '127.0.0.1';
  return { ...process.env, T12_BROWSER_PASSWORD: `T12 browser unique ${randomBytes(24).toString('hex')}`,  APP_ENV: 'test', DATABASE_URL: url.toString(), TEST_DATABASE_URL: url.toString(), REDIS_URL: redis.toString(), T12_SIGNING_KEY_FILE: resolve(root, '.local/signing.pem'), T12_ENCRYPTION_KEYS_FILE: keyPath, T12_ACTIVE_ENCRYPTION_KID: 't12-fixture', T12_SMTP_PASSWORD_FILE: smtpPasswordPath };
}
async function main() {
  const env = await prepare(); env.T12_CLIENT_SECRET_FILE = resolve(privateDirectory, 'client-secret');
  await writeFile(env.T12_CLIENT_SECRET_FILE, '', { mode: 0o600 });
  const baseArgs = ['test', '--package', 'identity-server', '--test', 't12_revocation', '--locked', '--', '--exact'];
  if (!e2e) {
  const result = await command('cargo', [...baseArgs, 't12_real_revocation', '--nocapture'], env);
  if (result.code !== 0) {
    const safe = `${result.stdout}${result.stderr}`.replaceAll(env.DATABASE_URL, '[DATABASE_URL]').replaceAll(env.T12_BROWSER_PASSWORD, '[PASSWORD]').replace(/\b[A-Za-z0-9_-]{43,}\b/gu, '[OPAQUE]');
    await mkdir(evidence, { recursive: true }); await writeFile(resolve(evidence, `integration-diagnostics-${Date.now()}.txt`), safe);
  }
  assert.equal(result.code, 0, 'T12 real token API failed; secret diagnostics suppressed.');
  assert.match(result.stdout, /1 passed/u); for (const line of result.stdout.split('\n')) if (line.startsWith('PASS T12')) lines.push(line);
  }
  const harness = spawnCapture('cargo', [...baseArgs, 't12_browser_harness', '--nocapture'], { ...env, T12_BROWSER_HARNESS: '1' }, 'pipe');
  try {
    let ready = false; for (let attempt = 0; attempt < 600; attempt += 1) { if (harness.output().includes('T12_BROWSER_READY')) { ready = true; break; } if (harness.child.exitCode !== null) break; await new Promise((resolveDelay) => setTimeout(resolveDelay, 100)); }
    assert.ok(ready, 'T12 interoperability harness failed to start.');
    const client = await command(process.execPath, e2e ? [resolve(root, 'node_modules/@playwright/test/cli.js'), 'test', '--config', 'tests/e2e/playwright.config.ts', '--grep', '@T12'] : [resolve(root, 'tests/interop/T12.mjs')], { ...env, E2E_TASK: 'T12' });
    if (client.code !== 0) {
      const safe = `${client.stdout}${client.stderr}`.replaceAll(env.T12_BROWSER_PASSWORD, '[PASSWORD]').replace(/\b[A-Za-z0-9_-]{43,}\b/gu, '[OPAQUE]');
      await writeFile(resolve(evidence, `interop-diagnostics-${Date.now()}.txt`), safe);
    }
    assert.equal(client.code, 0, 'Mature OIDC interoperability failed; secret diagnostics suppressed.');
    for (const line of client.stdout.split('\n')) if (line.startsWith('PASS T12')) lines.push(line);
    if (!e2e) {
      for (const dependency of ['redis', 'postgres']) {
        await compose('stop', dependency);
        try {
          const clientSecret = await (await import('node:fs/promises')).readFile(env.T12_CLIENT_SECRET_FILE, 'utf8');
          const auth = Buffer.from(`t12-a:${clientSecret}`).toString('base64');
          const response = await fetch('http://127.0.0.1:5191/oauth/introspect', { method: 'POST', headers: { Authorization: `Basic ${auth}`, 'Content-Type': 'application/x-www-form-urlencoded' }, body: 'token=unknown', signal: AbortSignal.timeout(15000) });
          assert.equal(response.status, 503, `${dependency} outage must return503 without a fabricated active flag.`);
          const body = await response.json(); assert.equal(body.error, 'temporarily_unavailable'); assert.equal(Object.hasOwn(body, 'active'), false);
          lines.push(`PASS T12 real ${dependency} container stopped: introspection503 and no fabricated active status.`);
        } finally { await compose('start', dependency); await compose('up', '-d', '--wait', dependency); }
      }
    }

    if (e2e) { assert.match(client.stdout, /2 passed/u); lines.push('Playwright: 2 passed / 0 failed; real grants and RP logout browser flows.'); }
  } finally { harness.child.stdin?.end('stop\n'); const stopped = await harness.completion; assert.equal(stopped.code, 0, 'T12 test schema cleanup failed.'); }
  console.log('T12 real token API and mature OIDC/JOSE interoperability passed without secret output.');
}
let passed = false;
try { await main(); passed = true; }
catch (error) { lines.push(`FAIL ${error.message}`); console.error(error.message); process.exitCode = 1; }
finally {
  lines.push(`Completed ${new Date().toISOString()}`, `Exit code ${passed ? 0 : 1}`);
  await mkdir(evidence, { recursive: true }); await writeFile(resolve(evidence, `${e2e ? 'e2e' : 'integration'}${passed ? '' : '-failure'}.txt`), `${lines.join('\n')}\n`);
  await rm(privateDirectory, { recursive: true, force: true });
}
