// Real IdP and same-package A/B/A2 BFFs. Private setup is test-only and schema isolated.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { randomBytes } from 'node:crypto';
import { resolve } from 'node:path';
import { localEnvironment, validateDatabaseTarget } from '../../scripts/database-target.mjs';
import { clearCurrentDiagnostic, sanitizeBrowserDiagnostic, writeCurrentDiagnostic } from '../../scripts/ci-e2e-diagnostics.mjs';
const root = resolve(import.meta.dirname, '../..');
const evidence = resolve(root, 'docs/evidence/T13');
const privateDirectory = resolve(root, '.local', `t13-${randomBytes(8).toString('hex')}`);
const lines = [`T13 real double-BFF browser/API integration ${new Date().toISOString()}`];
await clearCurrentDiagnostic(root, 'T13');
let currentStage = 'prepare'; let diagnosticWritten = false; let diagnosticEnvironment = process.env;
const capture = (program, args, env = process.env, input = 'ignore', cwd = root) => {
  const child = spawn(program, args, { cwd, env, shell: false, stdio: [input, 'pipe', 'pipe'] }); let stdout = '', stderr = '';
  child.stdout.setEncoding('utf8'); child.stderr.setEncoding('utf8'); child.stdout.on('data', (value) => { stdout += value; }); child.stderr.on('data', (value) => { stderr += value; });
  return { child, output: () => stdout + stderr, completion: new Promise((resolveResult, reject) => { child.on('error', () => reject(new Error(`${program} unavailable.`))); child.on('close', (code) => resolveResult({ code, stdout, stderr })); }) };
};
async function main() {
  const local = await localEnvironment(root); const supplied = process.env.TEST_DATABASE_URL ?? process.env.DATABASE_URL;
  if (supplied) validateDatabaseTarget(process.env.APP_ENV ?? 'test', supplied, { testOnly: true });
  const url = new URL(supplied ?? local.DATABASE_URL); if (!supplied) { url.hostname = '127.0.0.1'; url.pathname = '/identity_test'; url.search = ''; } validateDatabaseTarget('test', url.toString(), { testOnly: true });
  await mkdir(privateDirectory, { recursive: true, mode: 0o700 }); const keys = resolve(privateDirectory, 'keys.json'); await writeFile(keys, JSON.stringify({ 't13-fixture': randomBytes(32).toString('base64') }), { mode: 0o600 });
  const smtp = resolve(privateDirectory, 'smtp-password'); await writeFile(smtp, randomBytes(32).toString('hex'), { mode: 0o600 }); const redis = new URL(local.REDIS_URL); redis.hostname = '127.0.0.1';
  const env = { ...process.env, APP_ENV: 'test', DATABASE_URL: url.toString(), TEST_DATABASE_URL: url.toString(), REDIS_URL: redis.toString(), T13_BROWSER_PASSWORD: `T13 browser unique ${randomBytes(24).toString('hex')}`, T13_TEST_KEY: randomBytes(24).toString('hex'), T13_PRIVATE_DIRECTORY: privateDirectory, T13_SIGNING_KEY_FILE: resolve(root, '.local/signing.pem'), T13_ENCRYPTION_KEYS_FILE: keys, T13_ACTIVE_ENCRYPTION_KID: 't13-fixture', T13_SMTP_PASSWORD_FILE: smtp };
  diagnosticEnvironment = { ...env, T13_GENERATED_ENCRYPTION_SECRET: JSON.parse(await readFile(keys, 'utf8'))['t13-fixture'] };
  currentStage = 'startup';
  const webs = [];
  for (const [app, port, proxy] of [['identity-web', 5190, 5191], ['demo-a', 5192, 5194], ['demo-b', 5193, 5195]]) { webs.push(capture(process.execPath, [resolve(root, 'node_modules/vite/bin/vite.js'), '--host', '127.0.0.1', '--port', String(port), '--strictPort'], { ...env, IDENTITY_API_PROXY: `http://127.0.0.1:${proxy}`, BFF_API_PROXY: `http://127.0.0.1:${proxy}` }, 'ignore', resolve(root, 'apps', app))); }
  const harness = capture('cargo', ['test', '--package', 'identity-server', '--test', 't13_bff', '--locked', '--', '--exact', 't13_bff_harness', '--nocapture'], env, 'pipe');
  try {
    let ready = false; for (let attempt = 0; attempt < 600; attempt += 1) { if (harness.output().includes('T13_BFF_READY')) { ready = true; break; } if (harness.child.exitCode !== null) break; await new Promise((resolveDelay) => setTimeout(resolveDelay, 100)); }
    if (!ready) { const safe = sanitizeBrowserDiagnostic(harness.output(), diagnosticEnvironment); await mkdir(evidence, { recursive: true }); await writeFile(resolve(evidence, `harness-diagnostics-${Date.now()}.txt`), safe); await writeCurrentDiagnostic(root, 'T13', 'startup', safe, diagnosticEnvironment); diagnosticWritten = true; }
    assert.ok(ready, 'T13 same-package BFF harness failed to start; details sanitized.');
    currentStage = 'browser';
    const browser = capture(process.execPath, [resolve(root, 'node_modules/@playwright/test/cli.js'), 'test', '--config', 'tests/e2e/playwright.config.ts', '--grep', '@T13'], { ...env, E2E_TASK: 'T13', T13_WEBS_PRESTARTED: '1' }); const result = await browser.completion;
    if (result.code !== 0) { const safe = sanitizeBrowserDiagnostic(`${result.stdout}${result.stderr}`, diagnosticEnvironment); await writeFile(resolve(evidence, `browser-diagnostics-${Date.now()}.txt`), safe); await writeCurrentDiagnostic(root, 'T13', 'browser', safe, diagnosticEnvironment); diagnosticWritten = true; }
    assert.equal(result.code, 0, 'T13 real BFF browser checks failed.'); assert.match(result.stdout, /6 passed/u); lines.push('Playwright: 6 passed / 0 failed; actual A/B SSO, revocation, shared refresh and secret isolation.');
  } finally { harness.child.stdin?.end('stop\n'); const result = await harness.completion; for (const web of webs) web.child.kill(); if (result.code !== 0) { currentStage = 'cleanup'; await writeCurrentDiagnostic(root, 'T13', 'cleanup', `${result.stdout}${result.stderr}`, diagnosticEnvironment); diagnosticWritten = true; } assert.equal(result.code, 0, 'T13 isolated schema cleanup failed.'); }
  console.log('T13 actual A/B BFF and identity browser checks passed without token/client-secret output.');
}
let passed = false;
try { await main(); passed = true; } catch (error) { if (!diagnosticWritten) await writeCurrentDiagnostic(root, 'T13', currentStage, error.message, diagnosticEnvironment); lines.push(`FAIL ${error.message}`); console.error(error.message); process.exitCode = 1; }
finally { lines.push(`Completed ${new Date().toISOString()}`, `Exit code ${passed ? 0 : 1}`); await mkdir(evidence, { recursive: true }); await writeFile(resolve(evidence, `integration${passed ? '' : '-failure'}.txt`), `${lines.join('\n')}\n`); await rm(privateDirectory, { recursive: true, force: true }); }
