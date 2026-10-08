// Explicit test-only database plus real accounts/worker/SMTP. No trace or secret output.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { randomBytes } from 'node:crypto';
import { resolve } from 'node:path';
import { localEnvironment, validateDatabaseTarget } from '../../scripts/database-target.mjs';
import { sanitizeBrowserDiagnostic } from '../../scripts/ci-e2e-diagnostics.mjs';

const root = resolve(import.meta.dirname, '../..');
const e2e = process.argv.includes('--e2e');
const privateDirectory = resolve(root, '.local', `t05-${randomBytes(8).toString('hex')}`);
const evidence = resolve(root, 'docs/evidence/T05');
const lines = [`T05 ${e2e ? 'real UI E2E' : 'real account/SMTP integration'} ${new Date().toISOString()}`];
const composeArgs = ['compose', '--env-file', resolve(root, '.local/dev.env'), '-f', resolve(root, 'infra/compose.dev.yaml')];
let diagnosticEnvironment = process.env;
let fixturePasswords = [];
function safeDiagnostic(value) {
  let safe = sanitizeBrowserDiagnostic(value, diagnosticEnvironment);
  for (const password of fixturePasswords) {
    safe = safe.replaceAll(password, '[PASSWORD]');
    const bytes = [...Buffer.from(password, 'utf8')].map(String).join(',');
    if (bytes) safe = safe.replace(new RegExp(`\\[\\s*${bytes.replaceAll(',', ',\\s*')}\\s*\\]`, 'gu'), '[REDACTED PASSWORD BYTES]');
  }
  return safe
    .replace(/(#(?:token|code|state|nonce)=)[^&\s"'<>]+/giu, '$1[REDACTED]')
    .replace(/\b[0-9a-f]{8}-[0-9a-f-]{27}\b/giu, '[REDACTED UUID]');
}
async function preserveFailure(stage, result) {
  const stamp = new Date().toISOString().replaceAll(':', '-').replaceAll('.', '-');
  const safe = safeDiagnostic(`${result.stdout}${result.stderr}`);
  await mkdir(evidence, { recursive: true });
  await writeFile(resolve(evidence, `${e2e ? 'e2e' : 'integration'}-diagnostics-${stage}-${stamp}.txt`), `T05 current ${stage} failure; exit=${result.code}; generated=${new Date().toISOString()}\n${safe}\n`, { flag: 'wx' });
}
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
  await writeFile(keyPath, JSON.stringify({ 't05-fixture': key }), { mode: 0o600 });
  const redis = new URL(local.REDIS_URL); redis.hostname = '127.0.0.1';
  const rust = await readFile(resolve(root, 'crates/identity-server/tests/t05_accounts.rs'), 'utf8');
  const browser = await readFile(resolve(root, 'tests/e2e/T05.spec.ts'), 'utf8');
  fixturePasswords = [
    ...rust.matchAll(/const SAFE_PASSWORD: &str = "([^"]+)"/gu),
    ...rust.matchAll(/"password"\s*:\s*"([^"]+)"/gu),
    ...browser.matchAll(/getByLabel\('密码'[^\n]*?\.fill\('([^']+)'\)/gu),
  ].map((match) => match[1]);
  diagnosticEnvironment = { ...process.env, ...local, T05_GENERATED_ENCRYPTION_SECRET: key, T05_SIGNING_PRIVATE_KEY: await readFile(resolve(root, '.local/signing.pem'), 'utf8') };
  return { ...process.env, APP_ENV: 'test', DATABASE_URL: url.toString(), TEST_DATABASE_URL: url.toString(), REDIS_URL: redis.toString(), T05_SIGNING_KEY_FILE: resolve(root, '.local/signing.pem'), T05_ENCRYPTION_KEYS_FILE: keyPath, T05_ACTIVE_ENCRYPTION_KID: 't05-fixture' };
}
async function main() {
  const env = await prepare();
  diagnosticEnvironment = { ...diagnosticEnvironment, ...env };
  const args = ['test', '--package', 'identity-server', '--test', 't05_accounts', '--locked', '--', '--exact', e2e ? 't05_browser_harness' : 't05_real_accounts', '--nocapture'];
  if (!e2e) {
    const result = await command('cargo', args, env);
    if (result.code !== 0) await preserveFailure('rust', result);
    assert.equal(result.code, 0, 'T05 real integration failed; output suppressed to protect passwords/tokens.');
    assert.match(result.stdout, /1 passed/u); for (const line of result.stdout.split('\n')) if (line.startsWith('PASS T05')) lines.push(line);
  } else {
    const harness = spawnCapture('cargo', args, { ...env, T05_BROWSER_HARNESS: '1' }, 'pipe');
    try {
      let ready = false;
      for (let attempt = 0; attempt < 600; attempt += 1) {
        if (harness.output().includes('T05_BROWSER_READY')) { ready = true; break; }
        if (harness.child.exitCode !== null) break;
        await new Promise((resolveDelay) => setTimeout(resolveDelay, 100));
      }
      if (!ready) await preserveFailure('startup', { code: harness.child.exitCode ?? 1, stdout: harness.output(), stderr: '' });
      assert.ok(ready, 'T05 browser harness failed to start (diagnostics suppressed).');
      const result = await command(process.execPath, [resolve(root, 'node_modules/@playwright/test/cli.js'), 'test', '--config', 'tests/e2e/playwright.config.ts', '--grep', '@T05'], { ...env, IDENTITY_API_PROXY: 'http://127.0.0.1:5191' });
      if (result.code !== 0) {
        // Scrub transient diagnostics before persisting a failure report.
        await preserveFailure('browser', result);
      }
      assert.equal(result.code, 0, 'T05 browser checks failed; private details suppressed.');
      const passedMatch = /(?:^|\n)\s*(\d+) passed\b/u.exec(result.stdout);
      assert.equal(Number(passedMatch?.[1]), 2, 'T05 E2E must execute both real browser cases.');
      lines.push('Playwright: 2 passed / 0 failed; 2 task cases executed.');
      lines.push('PASS T05 real UI registration, Mailpit link, GET safety, confirmation and resend workflow');
    } finally {
      harness.child.stdin?.end('stop\n'); const result = await harness.completion;
      if (result.code !== 0) await preserveFailure('cleanup', result);
      assert.equal(result.code, 0, 'T05 harness cleanup failed; diagnostics suppressed.');
    }
  }
  console.log(`T05 ${e2e ? 'real browser' : 'real API/SMTP'} checks passed; token/password values and browser traces were suppressed.`);
}
let passed = false;
try { await main(); passed = true; }
catch (error) { lines.push(`FAIL ${error.message}`); console.error(error.message); process.exitCode = 1; }
finally {
  lines.push(`Completed ${new Date().toISOString()}`, `Exit code ${passed ? 0 : 1}`);
  await mkdir(evidence, { recursive: true }); await writeFile(resolve(evidence, `${e2e ? 'e2e' : 'integration'}${passed ? '' : '-failure'}.txt`), `${lines.join('\n')}\n`);
  await rm(privateDirectory, { recursive: true, force: true });
}
