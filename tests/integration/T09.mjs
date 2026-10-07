// Explicit test-only database plus real Passkey policy API. No trace or secret output.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdir, rm, writeFile } from 'node:fs/promises';
import { randomBytes } from 'node:crypto';
import { resolve } from 'node:path';
import { localEnvironment, validateDatabaseTarget } from '../../scripts/database-target.mjs';

const root = resolve(import.meta.dirname, '../..');
const e2e = process.argv.includes('--e2e');
const privateDirectory = resolve(root, '.local', `t09-${randomBytes(8).toString('hex')}`);
const evidence = resolve(root, 'docs/evidence/T09');
const lines = [`T09 ${e2e ? 'real UI E2E' : 'real Passkey policy integration'} ${new Date().toISOString()}`];
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
  await writeFile(keyPath, JSON.stringify({ 't09-fixture': key }), { mode: 0o600 });
  const smtpPasswordPath = resolve(privateDirectory, 'smtp-password');
  await writeFile(smtpPasswordPath, randomBytes(32).toString('hex'), { mode: 0o600 });
  const redis = new URL(local.REDIS_URL); redis.hostname = '127.0.0.1';
  return { ...process.env, T09_BROWSER_PASSWORD: `T09 browser unique ${randomBytes(24).toString('hex')}`,  APP_ENV: 'test', DATABASE_URL: url.toString(), TEST_DATABASE_URL: url.toString(), REDIS_URL: redis.toString(), T09_SIGNING_KEY_FILE: resolve(root, '.local/signing.pem'), T09_ENCRYPTION_KEYS_FILE: keyPath, T09_ACTIVE_ENCRYPTION_KID: 't09-fixture', T09_SMTP_PASSWORD_FILE: smtpPasswordPath };
}
async function main() {
  const env = await prepare();
  const args = ['test', '--package', 'identity-server', '--test', 't09_passkeys', '--locked', '--', '--exact', e2e ? 't09_browser_harness' : 't09_real_passkeys', '--nocapture'];
  if (!e2e) {
    const result = await command('cargo', args, env);
    if (result.code !== 0) {
      const safe = `${result.stdout}${result.stderr}`.replaceAll(env.DATABASE_URL, '[DATABASE_URL]').replaceAll(env.T09_BROWSER_PASSWORD, '[PASSWORD]').replace(/T09 distinct fixture long passphrase 78164/gu, '[PASSWORD]').replace(/\b[A-Za-z0-9_-]{43,}\b/gu, '[OPAQUE_DATA]');
      await mkdir(evidence, { recursive: true }); await writeFile(resolve(evidence, `integration-diagnostics-${Date.now()}.txt`), safe);
    }
    assert.equal(result.code, 0, 'T09 real integration failed; output suppressed to protect passwords/tokens.');
    assert.match(result.stdout, /1 passed/u); for (const line of result.stdout.split('\n')) if (line.startsWith('PASS T09')) lines.push(line);
  } else {
    const harness = spawnCapture('cargo', args, { ...env, T09_BROWSER_HARNESS: '1' }, 'pipe');
    try {
      let ready = false;
      for (let attempt = 0; attempt < 600; attempt += 1) {
        if (harness.output().includes('T09_BROWSER_READY')) { ready = true; break; }
        if (harness.child.exitCode !== null) break;
        await new Promise((resolveDelay) => setTimeout(resolveDelay, 100));
      }
      assert.ok(ready, 'T09 browser harness failed to start (diagnostics suppressed).');
      const result = await command(process.execPath, [resolve(root, 'node_modules/@playwright/test/cli.js'), 'test', '--config', 'tests/e2e/playwright.config.ts', '--grep', '@T09'], { ...env, E2E_TASK: 'T09', IDENTITY_API_PROXY: 'http://127.0.0.1:5191' });
      if (result.code !== 0) {
        // Scrub transient diagnostics before persisting a failure report.
        const safe = `${result.stdout}${result.stderr}`.replaceAll(env.T09_BROWSER_PASSWORD, '[PASSWORD]').replace(/#token=[A-Za-z0-9_-]+/gu, '#token=[REDACTED]').replace(/\b[A-Za-z0-9_-]{43,}\b/gu, '[OPAQUE_DATA]').replace(/[A-Za-z0-9_.+-]+@example\.test/gu, '[EMAIL]').replace(/T09 fixture/gu, '[PASSWORD]');
        await writeFile(resolve(evidence, 'e2e-diagnostics-sanitized.txt'), safe);
      }
      assert.equal(result.code, 0, 'T09 browser checks failed; private details suppressed.');
      const passedMatch = /(?:^|\n)\s*(\d+) passed\b/u.exec(result.stdout);
      assert.equal(Number(passedMatch?.[1]), 2, 'T09 E2E must execute both real browser cases.');
      lines.push('Playwright: 2 passed / 0 failed; 2 task cases executed.');
      lines.push('PASS T09 real UI virtual authenticator registration, discoverable login, reauthentication and rejection checks');
    } finally {
      harness.child.stdin?.end('stop\n'); const result = await harness.completion;
      assert.equal(result.code, 0, 'T09 harness cleanup failed; diagnostics suppressed.');
    }
  }
  console.log(`T09 ${e2e ? 'real browser' : 'real Passkey policy API'} checks passed; token/password values and browser traces were suppressed.`);
}
let passed = false;
try { await main(); passed = true; }
catch (error) { lines.push(`FAIL ${error.message}`); console.error(error.message); process.exitCode = 1; }
finally {
  lines.push(`Completed ${new Date().toISOString()}`, `Exit code ${passed ? 0 : 1}`);
  await mkdir(evidence, { recursive: true }); await writeFile(resolve(evidence, `${e2e ? 'e2e' : 'integration'}${passed ? '' : '-failure'}.txt`), `${lines.join('\n')}\n`);
  await rm(privateDirectory, { recursive: true, force: true });
}
