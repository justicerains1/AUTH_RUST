// Explicit test-only database plus real MFA API. No trace or secret output.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdir, rm, writeFile } from 'node:fs/promises';
import { randomBytes } from 'node:crypto';
import { resolve } from 'node:path';
import { localEnvironment, validateDatabaseTarget } from '../../scripts/database-target.mjs';

const root = resolve(import.meta.dirname, '../..');

const privateDirectory = resolve(root, '.local', `t18-${randomBytes(8).toString('hex')}`);
const evidence = resolve(root, 'docs/evidence/T18');
const lines = [`T18 real public-flow validation ${new Date().toISOString()}`];
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
  await writeFile(keyPath, JSON.stringify({ 't18-fixture': key }), { mode: 0o600 });
  const smtpPasswordPath = resolve(privateDirectory, 'smtp-password');
  await writeFile(smtpPasswordPath, randomBytes(32).toString('hex'), { mode: 0o600 });
  const redis = new URL(local.REDIS_URL); redis.hostname = '127.0.0.1';
  return { ...process.env, T18_CLOCK_KEY: randomBytes(24).toString('hex'), T18_NODE_EXEC: process.execPath, T18_BROWSER_PASSWORD: `T18 browser unique ${randomBytes(24).toString('hex')}`,  APP_ENV: 'test', DATABASE_URL: url.toString(), TEST_DATABASE_URL: url.toString(), REDIS_URL: redis.toString(), T18_SIGNING_KEY_FILE: resolve(root, '.local/signing.pem'), T18_ENCRYPTION_KEYS_FILE: keyPath, T18_ACTIVE_ENCRYPTION_KID: 't18-fixture', T18_SMTP_PASSWORD_FILE: smtpPasswordPath };
}
async function main() {
  const env = await prepare();
  const harness = spawnCapture('cargo', ['test', '--package', 'identity-server', '--test', 't18_account', '--locked', '--', '--exact', 't18_browser_harness', '--nocapture'], { ...env, T18_BROWSER_HARNESS: '1' }, 'pipe');
  try {
    let ready = false; for (let attempt = 0; attempt < 600; attempt += 1) { if (harness.output().includes('T18_BROWSER_READY')) { ready = true; break; } if (harness.child.exitCode !== null) break; await new Promise((resolveDelay) => setTimeout(resolveDelay, 100)); }
    if (!ready) { await mkdir(evidence,{recursive:true}); await writeFile(resolve(evidence,`harness-diagnostics-${Date.now()}.txt`),harness.output().replaceAll(env.DATABASE_URL,'[DATABASE]').replaceAll(env.T18_BROWSER_PASSWORD,'[PASSWORD]').replace(/\b[A-Za-z0-9_-]{43,}\b/gu,'[OPAQUE]')); }
    assert.ok(ready,'T18 real public-flow harness failed to start.');
    const result=await command(process.execPath,[resolve(root,'node_modules/@playwright/test/cli.js'),'test','--config','tests/e2e/playwright.config.ts','--grep','@T18'],{...env,E2E_TASK:'T18'});
    if(result.code!==0){await mkdir(evidence,{recursive:true});await writeFile(resolve(evidence,`browser-diagnostics-${Date.now()}.txt`),`${result.stdout}${result.stderr}`.replaceAll(env.T18_BROWSER_PASSWORD,'[PASSWORD]').replaceAll(env.T18_CLOCK_KEY,'[CLOCK_KEY]').replace(/\b[A-Z2-7]{32,128}\b/gu,'[TOTP_SECRET]').replace(/\b[A-Za-z0-9_-]{22,}\b/gu,'[OPAQUE]').replace(/\b\d{6}\b/gu,'[OTP]'));}
    assert.equal(result.code,0,'T18 real public-flow/accessibility tests failed.');assert.match(result.stdout,/4 passed/u);lines.push('Playwright:4 passed/0 failed; actual public flows and accessibility.');
  }finally{harness.child.stdin?.end('stop\n');const result=await harness.completion;assert.equal(result.code,0,'T18 isolated schema cleanup failed.');}
  console.log('T18 real public flows and accessibility passed without secret output.');
}
let passed = false;
try { await main(); passed = true; }
catch (error) { lines.push(`FAIL ${error.message}`); console.error(error.message); process.exitCode = 1; }
finally {
  lines.push(`Completed ${new Date().toISOString()}`, `Exit code ${passed ? 0 : 1}`);
  await mkdir(evidence, { recursive: true }); await writeFile(resolve(evidence, `e2e${passed ? '' : '-failure'}.txt`), `${lines.join('\n')}\n`);
  await rm(privateDirectory, { recursive: true, force: true });
}
