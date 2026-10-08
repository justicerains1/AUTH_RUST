// Local-only authenticated/anonymous ZAP checks against an isolated identity_test application.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdir, writeFile, rm } from 'node:fs/promises';
import { randomBytes } from 'node:crypto';
import { resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { localEnvironment, validateDatabaseTarget } from '../../scripts/database-target.mjs';

const root = resolve(import.meta.dirname, '../..');
const target = 'http://localhost:5290';
const zapOrigin = 'http://127.0.0.1:5299';
const evidence = resolve(root, 'docs/evidence/T20/zap');
const privateDirectory = resolve(root, '.local', `zap-${randomBytes(8).toString('hex')}`);
const report = { started: new Date().toISOString(), target: 'local isolated identity_test production build', version: '', phases: [], alerts: [], failures: [] };
function child(program, args, env, cwd = root) {
  const process = spawn(program, args, { cwd, env, shell: false, stdio: ['pipe', 'pipe', 'pipe'] });
  let output = ''; process.stdout.setEncoding('utf8'); process.stderr.setEncoding('utf8');
  process.stdout.on('data', (value) => { output += value; }); process.stderr.on('data', (value) => { output += value; });
  return { process, output: () => output, completion: new Promise((done, reject) => { process.on('error', reject); process.on('close', (code) => done(code)); }) };
}
async function zap(component, action, operation, parameters = {}) {
  const url = new URL(`/JSON/${component}/${action}/${operation}/`, zapOrigin);
  for (const [key, value] of Object.entries(parameters)) url.searchParams.set(key, String(value));
  const response = await fetch(url, { signal: AbortSignal.timeout(10_000) });
  if (!response.ok) throw new Error(`ZAP ${component}/${operation} failed.`);
  const value = await response.json();
  if (value.code) throw new Error(`ZAP ${component}/${operation} rejected operation (${value.code}).`);
  return value;
}
async function waitReady(instance, sentinel) {
  for (let attempt = 0; attempt < 600; attempt += 1) {
    if (instance.output().includes(sentinel)) return;
    if (instance.process.exitCode !== null) break;
    await delay(100);
  }
  throw new Error('Actual scan application did not become ready.');
}
async function main() {
  const local = await localEnvironment(root); const url = new URL(local.DATABASE_URL); url.hostname = '127.0.0.1'; url.pathname = '/identity_test'; url.search = '';
  validateDatabaseTarget('test', url.toString(), { testOnly: true });
  await mkdir(privateDirectory, { recursive: true, mode: 0o700 });
  const keys = resolve(privateDirectory, 'keys.json'); await writeFile(keys, JSON.stringify({ 'zap-fixture': randomBytes(32).toString('base64') }), { mode: 0o600 });
  const redis = new URL(local.REDIS_URL); redis.hostname = '127.0.0.1';
  const password = `ZAP isolated ${randomBytes(24).toString('hex')}`;
  const env = { ...process.env, APP_ENV: 'test', DATABASE_URL: url.toString(), TEST_DATABASE_URL: url.toString(), REDIS_URL: redis.toString(), T20_ZAP_HARNESS: '1', T20_ZAP_PASSWORD: password, T20_ZAP_SIGNING_KEY_FILE: resolve(root, '.local/signing.pem'), T20_ZAP_ENCRYPTION_KEYS_FILE: keys, T20_ZAP_ACTIVE_ENCRYPTION_KID: 'zap-fixture' };
  const harness = child('cargo', ['test', '-p', 'identity-server', '--test', 't20_zap', '--locked', '--', '--exact', 't20_zap_harness', '--nocapture'], env);
  const web = child(process.execPath, [resolve(root, 'tests/security/production-site.mjs')], env);
  try {
    await waitReady(harness, 'T20_ZAP_READY');
    await waitReady(web, 'PRODUCTION_SITE_READY');
    report.version = (await zap('core', 'view', 'version')).version; assert.equal(report.version, '2.17.0');
    await zap('core', 'action', 'newSession', { name: '', overwrite: true });
    await zap('core', 'action', 'deleteAllAlerts');
    for (const phase of ['anonymous', 'authenticated']) {
      if (phase === 'authenticated') {
        const response = await fetch(`${target}/api/v1/auth/csrf`); assert.equal(response.status, 200); const preauth = response.headers.getSetCookie().map((value) => value.split(';')[0]).join('; '); const csrf = (await response.json()).csrf_token;
        const login = await fetch(`${target}/api/v1/auth/login/password`, { method: 'POST', headers: { Origin: target, Cookie: preauth, 'X-CSRF-Token': csrf, 'Content-Type': 'application/json' }, body: JSON.stringify({ email: 'zap-user@example.test', password }) }); assert.equal(login.status, 200);
        const cookie = login.headers.getSetCookie().filter((value) => !value.includes('Max-Age=0')).map((value) => value.split(';')[0]).join('; ');
        await zap('replacer', 'action', 'addRule', { description: 'AUTH_RUST_LOCAL_COOKIE', enabled: true, matchType: 'REQ_HEADER', matchRegex: false, matchString: 'Cookie', replacement: cookie, url: '^http://localhost:5290/.*' });
      }
      await zap('core', 'action', 'accessUrl', { url: `${target}/login`, followRedirects: false });
      const access = await zap('core', 'action', 'accessUrl', { url: `${target}/api/v1/me`, followRedirects: false });
      const current = access.accessUrl?.[0];
      assert.ok(current, 'ZAP must actually request the protected API.');
      assert.match(current.responseHeader, phase === 'authenticated' ? /^HTTP\/\d(?:\.\d)? 200 /u : /^HTTP\/\d(?:\.\d)? 401 /u);
      for (const path of phase === 'authenticated' ? ['/me', '/api/v1/me', '/api/v1/me/sessions', '/api/v1/me/grants'] : ['/', '/register', '/password-reset', '/.well-known/openid-configuration', '/oauth/jwks']) await zap('core', 'action', 'accessUrl', { url: target + path, followRedirects: false });
      const spider = (await zap('spider', 'action', 'scan', { url: target, maxChildren: 10, recurse: true, subtreeOnly: true })).scan;
      for (let attempt = 0; attempt < 180; attempt += 1) { if ((await zap('spider', 'view', 'status', { scanId: spider })).status === '100') break; if (attempt === 179) throw new Error('ZAP spider deadline exceeded.'); await delay(500); }
      await zap('ascan', 'action', 'setOptionMaxScanDurationInMins', { Integer: 2 });
      const scan = (await zap('ascan', 'action', 'scan', { url: `${target}/api/v1/me`, recurse: false, inScopeOnly: false, scanPolicyName: 'API' })).scan;
      for (let attempt = 0; attempt < 400; attempt += 1) { if ((await zap('ascan', 'view', 'status', { scanId: scan })).status === '100') break; if (attempt === 399) throw new Error('ZAP active scan deadline exceeded.'); await delay(500); }
      for (let attempt = 0; attempt < 120; attempt += 1) { if ((await zap('pscan', 'view', 'recordsToScan')).recordsToScan === '0') break; await delay(250); }
      report.phases.push({ phase, spider: 'complete', active: 'complete', targetPath: '/api/v1/me', confirmedProtectedStatus: phase === 'authenticated' ? 200 : 401 });
      if (phase === 'authenticated') await zap('replacer', 'action', 'removeRule', { description: 'AUTH_RUST_LOCAL_COOKIE' });
    }
    const alerts = (await zap('core', 'view', 'alerts', { baseurl: target, start: 0, count: 1000 })).alerts;
    report.alerts = alerts.map(({ pluginId, name, risk, confidence, url, description, solution }) => ({ pluginId, name, risk, confidence, path: new URL(url).pathname, description, solution }));
    if (report.alerts.some((alert) => ['High', 'Critical'].includes(alert.risk))) report.failures.push('ZAP reported a High/Critical finding.');
  } finally {
    await zap('replacer', 'action', 'removeRule', { description: 'AUTH_RUST_LOCAL_COOKIE' }).catch(() => {});
    harness.process.stdin.end('stop\n'); const code = await harness.completion; web.process.kill();
    if (code !== 0) report.failures.push('Isolated scan schema cleanup failed.');
  }
}
try { await main(); } catch (error) { report.failures.push(error.message); }
finally {
  report.completed = new Date().toISOString(); report.exitCode = report.failures.length ? 1 : 0;
  await mkdir(evidence, { recursive: true }); await writeFile(resolve(evidence, `scan-${Date.now()}.json`), JSON.stringify(report, null, 2)+'\n');
  await rm(privateDirectory, { recursive: true, force: true }); console.log(`ZAP local scan exit ${report.exitCode}; sanitized report saved.`); process.exitCode = report.exitCode;
}
