// Dedicated real IdP/PG/Mailpit product checks; no response mocking and no shared dependency stop.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { randomBytes, createHash } from 'node:crypto';
import { mkdir, readFile, writeFile, rm } from 'node:fs/promises';
import { resolve, relative } from 'node:path';
import { chromium, firefox } from 'playwright';
import { localEnvironment, validateDatabaseTarget } from '../../scripts/database-target.mjs';
const root = resolve(import.meta.dirname, '../..');
const privateDirectory = resolve(root, '.local', `product-accessibility-${randomBytes(8).toString('hex')}`);
const evidence = resolve(root, 'docs/evidence/T23/product-accessibility');
const timestamp = new Date().toISOString().replaceAll(':', '-').replaceAll('.', '-');
const focused = process.argv.includes('--focus-client-dialogs');
assert.ok(process.argv.slice(2).every((argument) => argument === '--focus-client-dialogs'), 'Unknown product accessibility argument.');
const expectedCases = focused ? 2 : 12;
const report = { started: new Date().toISOString(), scope: 'Real Chromium and Linux Playwright Firefox automation. Does not prove screen-reader speech, physical mobile/Passkey hardware, or Safari compatibility.', browsers: [], results: {}, exitCode: 1 };
let harness;
function capture(program, args, env = process.env, input = 'ignore') {
  const child = spawn(program, args, { cwd: root, env, shell: false, stdio: [input, 'pipe', 'pipe'] }); let output = '';
  child.stdout.setEncoding('utf8'); child.stderr.setEncoding('utf8'); child.stdout.on('data', (value) => { output += value; }); child.stderr.on('data', (value) => { output += value; });
  const completion = new Promise((done) => { child.on('error', () => done({ code: 127, output: 'Required test tool unavailable.' })); child.on('close', (code) => done({ code, output })); });
  return { child, completion, output: () => output };
}
async function main() {
  const local = await localEnvironment(root); const supplied = process.env.TEST_DATABASE_URL ?? process.env.DATABASE_URL;
  if (supplied) validateDatabaseTarget(process.env.APP_ENV ?? 'test', supplied, { testOnly: true });
  const database = new URL(supplied ?? local.DATABASE_URL); if (!supplied) { database.hostname = '127.0.0.1'; database.pathname = '/identity_test'; database.search = ''; }
  validateDatabaseTarget('test', database.toString(), { testOnly: true });
  await mkdir(privateDirectory, { recursive: true, mode: 0o700 }); await mkdir(evidence, { recursive: true });
  const keys = resolve(privateDirectory, 'keys.json'); await writeFile(keys, JSON.stringify({ 'access-fixture': randomBytes(32).toString('base64') }), { mode: 0o600 });
  const redis = new URL(local.REDIS_URL); redis.hostname = '127.0.0.1';
  const env = { ...process.env, APP_ENV: 'test', DATABASE_URL: database.toString(), TEST_DATABASE_URL: database.toString(), REDIS_URL: redis.toString(), T23_ACCESSIBILITY_BROWSER_HARNESS: '1', T23_ACCESSIBILITY_CLOCK_KEY: randomBytes(24).toString('hex'), T23_ACCESSIBILITY_BROWSER_PASSWORD: `Product check distinct ${randomBytes(24).toString('hex')}`, T23_ACCESSIBILITY_SIGNING_KEY_FILE: resolve(root, '.local/signing.pem'), T23_ACCESSIBILITY_ENCRYPTION_KEYS_FILE: keys, T23_ACCESSIBILITY_ACTIVE_ENCRYPTION_KID: 'access-fixture', PRODUCT_ACCESSIBILITY_EVIDENCE: evidence };
  for (const [name, launcher] of [['chromium', chromium], ['firefox', firefox]]) { const browser = await launcher.launch(); report.browsers.push({ name, version: browser.version(), executableSha256: createHash('sha256').update(await readFile(launcher.executablePath())).digest('hex') }); await browser.close(); }
  harness = capture(process.env.PRODUCT_CARGO ?? 'cargo', ['test', '--package', 'identity-server', '--test', 't23_accessibility', '--locked', '--', '--exact', 't23_product_accessibility_harness', '--nocapture'], env, 'pipe');
  let ready = false; for (let count = 0; count < 1200; count += 1) { if (harness.output().includes('T23_ACCESSIBILITY_BROWSER_READY')) { ready = true; break; } if (harness.child.exitCode !== null) break; await new Promise((done) => setTimeout(done, 100)); }
  const safe = (text) => text.replaceAll(env.T23_ACCESSIBILITY_BROWSER_PASSWORD, '[PASSWORD]').replaceAll(env.T23_ACCESSIBILITY_CLOCK_KEY, '[CONTROL_KEY]').replaceAll(env.DATABASE_URL, '[DATABASE_URL]').replace(/\b[A-Z2-7]{32,128}\b/gu, '[TOTP_SECRET]').replace(/\b[A-Za-z0-9_-]{22,}\b/gu, '[OPAQUE]').replace(/#token=[^\s"']+/gu, '#token=[REDACTED]').replace(/([?&](?:code|nonce|state)=)[^&\s"']+/gu, '$1[REDACTED]');
  if (!ready) await writeFile(resolve(evidence, `${timestamp}-harness-failure.txt`), safe(harness.output()));
  assert.ok(ready, 'Isolated product accessibility identity harness did not start.');
  const browser = capture(process.execPath, [resolve(root, 'node_modules/@playwright/test/cli.js'), 'test', '--config', 'tests/e2e/product-accessibility.config.ts', ...(focused ? ['--grep', 'client secret dialogs and strong authentication keyboard sequence'] : [])], env);
  const result = await browser.completion; await writeFile(resolve(privateDirectory, 'browser-output.txt'), result.output, { mode: 0o600 });
  if (result.code !== 0) await writeFile(resolve(evidence, `${timestamp}-browser-failure.txt`), safe(result.output));
  report.results = { browserExitCode: result.code, suites: 2, expectedCases, focusedClientDialogsOnly: focused };
  assert.equal(result.code, 0, 'Real product accessibility or Firefox checks failed.'); assert.match(result.output, new RegExp(`${String(expectedCases)} passed`, 'u'));
  report.exitCode = 0;
}
try { await main(); } catch { report.failure = 'Actual product checks did not complete or failed; see sanitized diagnostics. No private API payload or credentials are printed.'; }
finally {
  if (harness !== undefined) { harness.child.stdin?.end('stop\n'); const result = await harness.completion; if (result.code !== 0) { report.exitCode = 1; report.cleanupFailure = true; } }
  report.completed = new Date().toISOString(); const file = resolve(evidence, `${timestamp}${report.exitCode === 0 ? '' : '-failure'}.json`); await mkdir(evidence, { recursive: true }); await writeFile(file, `${JSON.stringify(report, null, 2)}\n`);
  await rm(privateDirectory, { recursive: true, force: true }); console.log(`Product accessibility/Firefox ${report.exitCode === 0 ? 'automatic checks passed' : 'checks failed'}; ${relative(root, file)}. Physical-device and screen-reader acceptance unchanged.`); process.exitCode = report.exitCode;
}
