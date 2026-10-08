import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { createHash, randomBytes } from 'node:crypto';
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { cpus, totalmem, platform, release } from 'node:os';
import { resolve, relative } from 'node:path';
import { localEnvironment, validateDatabaseTarget } from '../../scripts/database-target.mjs';

const root = resolve(import.meta.dirname, '../..');
const args = process.argv.slice(2);
assert.ok(args.length === 1 && /^(?:--scenario=(?:introspection|account|password|mixed)|--seed-only|--probe-only)$/u.test(args[0]), 'Usage: node tests/load/run.mjs --scenario=introspection|account|password|mixed or --seed-only or --probe-only');
const scenario = args[0].startsWith('--scenario=') ? args[0].slice(11) : undefined;
const probeOnly = args[0] === '--probe-only';
const started = new Date(); const runId = started.toISOString().replaceAll(':', '-').replaceAll('.', '-');
const privateDirectory = resolve(root, '.local', `t21-load-${randomBytes(8).toString('hex')}`);
const evidence = resolve(root, 'docs/evidence/T21');
const k6 = resolve(root, '.local/t21-tools/k6-v2.3.0-linux-amd64/k6');
const k6Archive = resolve(root, '.local/t21-tools/k6-v2.3.0-linux-amd64.tar.gz');
const expectedArchiveSha256 = '39c3117b6af817592dcd0ce4242105c0a7af10948c2a425306f0be8f7a8a8ab1';
const report = { kind: 'T21 load independent submodule, not overall acceptance', scenario: scenario ?? (probeOnly ? 'sql-probe' : 'seed-only'), started: started.toISOString(), environment: { os: `${platform()} ${release()}`, cpuModel: cpus()[0]?.model, logicalCpus: cpus().length, memoryGiB: totalmem() / 1024 ** 3, referenceMatches: false, note: 'Same-host WSL2 Ryzen7700X16logical15.2GiB differs from Linux8vCPU16GiBSSD reference; load generator shares CPU with API/PG/Redis.' }, warmupSeconds: scenario ? 120 : 0, measurementSeconds: scenario ? 900 : 0, resources: [], errors: [] };
let harness; let monitor; let k6Process;
function capture(command, params, env = process.env, stdin = 'ignore') { const child = spawn(command, params, { cwd: root, env, shell: false, stdio: [stdin, 'pipe', 'pipe'] }); let output = ''; child.stdout.setEncoding('utf8'); child.stderr.setEncoding('utf8'); child.stdout.on('data', (part) => { output += part; }); child.stderr.on('data', (part) => { output += part; }); const completion = new Promise((done) => { child.on('error', () => done({code:127,output:'Required load tool unavailable.'})); child.on('close', (code) => done({ code, output })); }); return { child, completion, output: () => output }; }
function fail(message) { report.errors.push(message); }
async function stats(env) {
  try {
    const response = await fetch('http://127.0.0.1:5311/stats', { headers: { 'x-test-key': env.T21_CONTROL_KEY }, signal: AbortSignal.timeout(4000) });
    const service = response.status === 200 ? await response.json() : { unavailable: true };
    if (service.unavailable) fail('Actual SQL/queue resource observation unavailable.');
    const endpoints = service.query_observation?.endpoints;
    if (endpoints && Object.values(endpoints).some((entry) => entry.unknown_statement_count !== 0)) fail('Unknown SQL shape or missing completion timing invalidates query observation.');
    const docker = capture('docker', ['stats', '--no-stream', '--format', '{{json .}}', 'auth-rust-development-postgres-1', 'auth-rust-development-redis-1']); const result = await docker.completion;
    const containers = result.code === 0 ? result.output.trim().split('\n').filter(Boolean).map((line) => JSON.parse(line)) : [];
    let apiProcess = {}; try { const children = (await readFile(`/proc/${harness.child.pid}/task/${harness.child.pid}/children`, 'utf8')).trim().split(' '); const pid = children[0]; const fields = (await readFile(`/proc/${pid}/status`, 'utf8')).split('\n').filter((line) => /^(?:VmRSS|VmHWM|Threads):/u.test(line)); const stat = (await readFile(`/proc/${pid}/stat`, 'utf8')).split(' '); apiProcess = { fields, userCpuTicks: Number(stat[13]), systemCpuTicks: Number(stat[14]) }; } catch { /* Completed harness is reported by completion checks. */ }
    let generator = {}; try { const pid = k6Process?.child.pid; if (pid !== undefined) { const fields = (await readFile(`/proc/${pid}/status`, 'utf8')).split('\n').filter((line) => /^(?:VmRSS|VmHWM|Threads):/u.test(line)); const stat = (await readFile(`/proc/${pid}/stat`, 'utf8')).split(' '); generator = { fields, userCpuTicks: Number(stat[13]), systemCpuTicks: Number(stat[14]) }; } } catch { /* k6 completed. */ }
    report.resources.push({ at: new Date().toISOString(), service, containers, apiHarness: apiProcess, loadGenerator: generator });
  } catch { report.resources.push({ at: new Date().toISOString(), unavailable: true }); fail('Actual SQL/queue resource observation failed.'); }
}
async function main() {
  const environment = process.env.APP_ENV ?? 'test'; const supplied = process.env.TEST_DATABASE_URL ?? process.env.DATABASE_URL;
  if (supplied) { assert.ok(new URL(supplied).search === '', 'Load rejects database target query overrides.'); validateDatabaseTarget(environment, supplied, { testOnly: true }); } else assert.equal(environment, 'test', 'Load fixture requires test environment.');
  const local = await localEnvironment(root); const database = new URL(supplied ?? local.DATABASE_URL); if (!supplied) { database.hostname = '127.0.0.1'; database.pathname = '/identity_test'; database.search = ''; }
  validateDatabaseTarget('test', database.toString(), { testOnly: true });
  assert.ok(['localhost', '127.0.0.1', '[::1]'].includes(database.hostname), 'Load fixture refuses non-loopback database hosts.');
  if (scenario !== undefined) assert.equal(createHash('sha256').update(await readFile(k6Archive)).digest('hex'), expectedArchiveSha256, 'Official exact k6 archive checksum mismatch.');
  await mkdir(privateDirectory, { recursive: true, mode: 0o700 }); const keys = resolve(privateDirectory, 'keys.json'); await writeFile(keys, JSON.stringify({ 't21-fixture': randomBytes(32).toString('base64') }), { mode: 0o600 });
  const redis = new URL(local.REDIS_URL); redis.hostname = '127.0.0.1';
  const env = { ...process.env, APP_ENV: 'test', DATABASE_URL: database.toString(), TEST_DATABASE_URL: database.toString(), REDIS_URL: redis.toString(), T21_LOAD_HARNESS: '1', T21_PRIVATE_DIRECTORY: privateDirectory, T21_PASSWORD: `T21 real password ${randomBytes(24).toString('hex')}`, T21_CONTROL_KEY: randomBytes(24).toString('hex'), T21_SIGNING_KEY_FILE: resolve(root, '.local/signing.pem'), T21_ENCRYPTION_KEYS_FILE: keys, T21_ACTIVE_ENCRYPTION_KID: 't21-fixture' };
  harness = capture(process.env.T21_CARGO ?? '/root/.cargo/bin/cargo', ['test', '--release', '-p', 'identity-server', '--test', 't21_load', '--locked', '--', '--exact', 't21_load_harness', '--nocapture'], env, 'pipe');
  let ready = false; for (let attempt = 0; attempt < 6000; attempt += 1) { if (harness.output().includes('T21_LOAD_READY')) { ready = true; break; } if (harness.child.exitCode !== null) break; await new Promise((done) => setTimeout(done, 100)); }
  if (!ready) { const safe = harness.output().replaceAll(env.DATABASE_URL, '[DATABASE]').replaceAll(env.T21_PASSWORD, '[PASSWORD]').replaceAll(env.T21_CONTROL_KEY, '[CONTROL_KEY]').replace(/\b[A-Za-z0-9_-]{43,}\b/gu, '[OPAQUE]').replace(/\b[0-9a-f]{8}-[0-9a-f-]{27}\b/giu, '[REDACTED UUID]').replace(/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+/gu, '[REDACTED EMAIL]'); await writeFile(resolve(evidence, `${probeOnly ? 'sql-probe' : 'load'}-${runId}-harness-failure.txt`), safe); }
  assert.ok(ready, 'Actual isolated load harness failed to become ready.');
  report.seed = JSON.parse(await readFile(resolve(privateDirectory, 'seed-summary.json'), 'utf8'));
  report.queryPlans = JSON.parse(await readFile(resolve(privateDirectory, 'query-plans.json'), 'utf8'));
  report.queryEvidence = JSON.parse(await readFile(resolve(privateDirectory, 'query-evidence.json'), 'utf8'));
  assert.equal(report.seed.users, 100000); assert.equal(report.seed.clients, 20); assert.equal(report.seed.active_grants, 100000);
  report.environment.git = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim();
  report.sourceSha256 = {};
  for (const path of ['tests/load/run.mjs', 'tests/load/scenarios.js', 'tests/load/query-evidence.rs', 'crates/identity-server/tests/t21_load.rs', 'crates/identity-store/src/sessions.rs', 'crates/identity-store/src/tokens.rs', 'crates/identity-store/src/security.rs', 'crates/identity-store/src/repository.rs', 'Cargo.lock']) report.sourceSha256[path] = createHash('sha256').update(await readFile(resolve(root, path))).digest('hex');
  try { const pid = (await readFile(`/proc/${harness.child.pid}/task/${harness.child.pid}/children`, 'utf8')).trim().split(' ')[0]; report.apiExecutableSha256 = createHash('sha256').update(await readFile(`/proc/${pid}/exe`)).digest('hex'); } catch { fail('API binary fingerprint unavailable.'); }
  await stats(env);
  if (scenario !== undefined) {
    const version = capture(k6, ['version']); const verified = await version.completion; assert.equal(verified.code, 0); assert.match(verified.output, /k6 v2\.3\.0/u); report.k6 = verified.output.trim();
    report.k6ExecutableSha256 = createHash('sha256').update(await readFile(k6)).digest('hex');
    const summary = resolve(privateDirectory, 'summary.json');
    k6Process = capture(k6, ['run', '--quiet', '--no-color', resolve(root, 'tests/load/scenarios.js')], { ...env, T21_CREDENTIALS: resolve(privateDirectory, 'credentials.json'), T21_SCENARIO: scenario, T21_SUMMARY: summary });
    monitor = setInterval(() => { void stats(env); }, 15000);
    const result = await k6Process.completion;
    report.k6ExitCode = result.code; report.summary = JSON.parse(await readFile(summary, 'utf8'));
    report.actualMeasurementRequests = report.summary.metrics.measurement_requests?.values?.count ?? 0;
    report.actualAverageMeasurementRps = report.actualMeasurementRequests / 900;
    if (result.code !== 0) fail(result.code === 105 ? 'Interrupted before completing the required measurement; not a full performance result.' : 'k6 measurements did not meet all documented thresholds or could not sustain configured rate.');
    const refreshFailures = report.resources.some((sample) => (sample.service?.refresh_failures ?? 0) !== 0);
    if (refreshFailures) fail('Controlled token pool renewal failed; validity cannot be assumed.');
    await stats(env);
    const safe = result.output.replaceAll(env.T21_PASSWORD, '[PASSWORD]').replaceAll(env.T21_CONTROL_KEY, '[CONTROL_KEY]').replace(/\b[A-Za-z0-9_-]{43,}\b/gu, '[OPAQUE]').replace(/(token=)[^\s&"']+/gu, '$1[REDACTED]'); await writeFile(resolve(evidence, `load-${scenario}-${runId}-output.txt`), safe);
  }
}
await mkdir(evidence, { recursive: true });
try { await main(); } catch (error) { fail(error instanceof assert.AssertionError ? error.message.split('\n')[0] : 'Load measurement did not complete; check sanitized harness report and exact local target.'); }
finally {
  clearInterval(monitor); k6Process?.child.kill();
  if (harness !== undefined) { harness.child.stdin?.end('stop\n'); const result = await harness.completion; if (result.code !== 0) fail('Load harness or own schema cleanup failed.'); }
  report.completed = new Date().toISOString(); report.exitCode = report.errors.length === 0 ? 0 : 1;
  const file = resolve(evidence, `${probeOnly ? 'sql-probe' : `load-${report.scenario}`}-${runId}${report.exitCode === 0 ? '' : '-failure'}.json`); await writeFile(file, `${JSON.stringify(report, null, 2)}\n`, { flag: 'wx' }); await rm(privateDirectory, { recursive: true, force: true });
  console.log(`T21 ${report.scenario} ${report.exitCode === 0 ? 'submodule completed' : 'measurement failed'}; report ${relative(root, file)}. Overall T21 status is unchanged.`); process.exitCode = report.exitCode;
}
