// Real Compose/PostgreSQL tests. Only identity_test and generated schemas may mutate.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { localEnvironment, validateDatabaseTarget } from '../../scripts/database-target.mjs';

const root = resolve(import.meta.dirname, '../..');
const evidence = resolve(root, 'docs/evidence/T03');
const lines = [`T03 real database acceptance: ${new Date().toISOString()}`];
const composePrefix = ['compose', '--env-file', resolve(root, '.local/dev.env'), '-f', resolve(root, 'infra/compose.dev.yaml')];

function processResult(program, args, environment = process.env) {
  return new Promise((resolveResult, reject) => {
    const child = spawn(program, args, { cwd: root, env: environment, shell: false, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = ''; let stderr = '';
    child.stdout.setEncoding('utf8'); child.stderr.setEncoding('utf8');
    child.stdout.on('data', (data) => { stdout += data; }); child.stderr.on('data', (data) => { stderr += data; });
    child.on('error', () => reject(new Error(`${program} unavailable; install and enable the documented tool.`)));
    child.on('close', (code) => resolveResult({ code, stdout, stderr }));
  });
}

async function compose(...args) {
  const result = await processResult('docker', [...composePrefix, ...args]);
  assert.equal(result.code, 0, `Compose ${args[0]} failed; child diagnostics suppressed.`);
  return result;
}

async function main() {
  // User-supplied unsafe target is rejected before even inspecting Docker.
  const appEnv = process.env.APP_ENV ?? 'test';
  if (process.env.TEST_DATABASE_URL || process.env.DATABASE_URL) {
    validateDatabaseTarget(appEnv, process.env.TEST_DATABASE_URL ?? process.env.DATABASE_URL, { testOnly: true });
  } else assert.equal(appEnv, 'test', 'T03 requires test environment before any connection');
  const local = await localEnvironment(root);
  let databaseUrl = process.env.TEST_DATABASE_URL ?? process.env.DATABASE_URL;
  if (!databaseUrl) {
    const url = new URL(local.DATABASE_URL); url.hostname = '127.0.0.1'; url.pathname = '/identity_test'; url.search = '';
    databaseUrl = url.toString();
  }
  validateDatabaseTarget('test', databaseUrl, { testOnly: true });
  const stateRaw = (await compose('ps', '--all', '--format', 'json')).stdout.trim();
  const states = stateRaw.startsWith('[') ? JSON.parse(stateRaw) : stateRaw.split('\n').filter(Boolean).map((line) => JSON.parse(line));
  for (const name of ['postgres', 'redis', 'api']) {
    const state = states.find((entry) => entry.Service === name);
    assert.ok(state && state.State === 'running' && state.Health === 'healthy', `${name} must be running/healthy; run npm run dev:up.`);
  }
  const redis = await compose('exec', '-T', 'redis', 'redis-cli', 'PING'); assert.equal(redis.stdout.trim(), 'PONG');
  const ready = await fetch('http://127.0.0.1:8080/health/ready', { signal: AbortSignal.timeout(5000) });
  assert.equal(ready.status, 200); assert.deepEqual(await ready.json(), { status: 'ready' });
  lines.push('PASS real Redis PING and API readiness against PG+Redis');
  const existing = await compose('exec', '-T', 'postgres', 'psql', '-U', 'identity', '-d', 'postgres', '-Atc', "SELECT datname FROM pg_database WHERE datname='identity_test'");
  if (!existing.stdout.trim()) await compose('exec', '-T', 'postgres', 'createdb', '-U', 'identity', 'identity_test');
  lines.push('PASS explicit test target identity_test exists; no non-test database cleanup');
  const environment = { ...process.env, APP_ENV: 'test', TEST_DATABASE_URL: databaseUrl, DATABASE_URL: databaseUrl };
  const args = ['test', '--package', 'identity-store', '--test', 't03_database', '--locked', '--', '--nocapture'];
  const result = await processResult('cargo', args, environment);
  assert.equal(result.code, 0, 'Real database suite failed; raw child diagnostics suppressed to protect credentials.');
  for (const line of result.stdout.split('\n')) if (line.startsWith('PASS T03') || line.startsWith('PASS transaction')) lines.push(line);
  assert.ok(lines.some((line) => line.startsWith('PASS T03-DB-03')), 'Missing database concurrency results');
  // Test guard rejection uses a non-routable connection; rejection must occur before connecting.
  for (const [name, appEnv, url] of [
    ['production', 'production', 'postgres://identity:REDACTION_SENTINEL@127.0.0.1:1/identity_test'],
    ['wrong database', 'test', 'postgres://identity:REDACTION_SENTINEL@127.0.0.1:1/identity_production'],
    ['driver dbname override', 'test', 'postgres://identity:REDACTION_SENTINEL@127.0.0.1:1/identity_test?dbname=identity_production'],
  ]) {
    const rejected = await processResult('cargo', args, { ...environment, APP_ENV: appEnv, TEST_DATABASE_URL: url });
    assert.notEqual(rejected.code, 0, `${name} test target unexpectedly accepted`);
    assert.ok(!`${rejected.stdout}${rejected.stderr}`.includes('REDACTION_SENTINEL'), 'Rejected target leaked credentials');
    lines.push(`PASS T03-DB-04 ${name}: nonzero before connection; no mutations`);
  }
  console.log('T03 real database migration, constraints, concurrency, rollback, expiry and target rejection passed.');
}

let passed = false;
try { await main(); passed = true; }
catch (error) { lines.push(`FAIL ${error.message}`); console.error(error.message); process.exitCode = 1; }
finally {
  lines.push(`Completed: ${new Date().toISOString()}`, `Exit code: ${passed ? 0 : 1}`);
  await mkdir(evidence, { recursive: true });
  await writeFile(resolve(evidence, passed ? 'integration.txt' : 'integration-failure.txt'), `${lines.join('\n')}\n`);
}
