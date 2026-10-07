// Real Compose services only. No mock transports and no successful missing-test path.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { chmod, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { randomBytes } from 'node:crypto';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { dirname } from 'node:path';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const composeArgs = ['compose', '--env-file', resolve(root, '.local/dev.env'), '-f', resolve(root, 'infra/compose.dev.yaml')];
const evidence = resolve(root, 'docs/evidence/T01');
const lines = [`# T01 real service acceptance`, `Started: ${new Date().toISOString()}`];
const privateTestDir = resolve(root, '.local/t01-config-tests');

function run(args, env = process.env) {
  return new Promise((resolveResult, reject) => {
    const child = spawn('docker', args, { cwd: root, env, shell: false, stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = ''; let stderr = '';
    child.stdout.setEncoding('utf8'); child.stderr.setEncoding('utf8');
    child.stdout.on('data', (data) => { stdout += data; });
    child.stderr.on('data', (data) => { stderr += data; });
    child.on('error', () => reject(new Error('Docker is unavailable; install and start Docker Compose v2.')));
    child.on('close', (code) => resolveResult({ code, stdout, stderr }));
  });
}

async function compose(...args) {
  const result = await run([...composeArgs, ...args]);
  assert.equal(result.code, 0, `Compose ${args[0]} failed (details suppressed to avoid secret leakage)`);
  return result;
}

async function health(expected, attempts = 30) {
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    try {
      const live = await fetch('http://localhost:8080/health/live', { signal: AbortSignal.timeout(5000) });
      const ready = await fetch('http://localhost:8080/health/ready', { signal: AbortSignal.timeout(5000) });
      assert.equal(live.status, 200);
      assert.deepEqual(await live.json(), { status: 'live' });
      if (ready.status === expected) {
        assert.deepEqual(await ready.json(), { status: expected === 200 ? 'ready' : 'unavailable' });
        return;
      }
    } catch (error) {
      if (attempt === attempts - 1) throw new Error(`Health request failed: ${error.name}`, { cause: error });
    }
    // Poll only actual service startup/recovery; not authentication expiry.
    await delay(500);
  }
  throw new Error(`Readiness did not reach ${expected}.`);
}

async function configCases() {
  await mkdir(privateTestDir, { recursive: true, mode: 0o700 });
  await chmod(privateTestDir, 0o700);
  const passwordPath = resolve(privateTestDir, 'smtp-password');
  const smtpPassword = randomBytes(32).toString('hex');
  await writeFile(passwordPath, smtpPassword, { mode: 0o600 });
  const baseline = {
    APP_ENV: 'production',
    BIND: '127.0.0.1:8080',
    ISSUER: 'https://identity.example',
    RP_ID: 'identity.example',
    DATABASE_URL: `postgres://identity:${randomBytes(32).toString('hex')}@postgres:5432/identity_test`,
    REDIS_URL: 'redis://redis:6379',
    SIGNING_KEY_FILE: '/run/secrets/signing.pem',
    SIGNING_KID: 'local-signing-1',
    ENCRYPTION_KEYS_FILE: '/run/secrets/encryption-keys.json',
    ACTIVE_ENCRYPTION_KID: 'local-aead-1',
    SMTP_HOST: 'smtp.example',
    SMTP_PORT: '587',
    SMTP_USERNAME: `test-${randomBytes(16).toString('hex')}`,
    SMTP_PASSWORD_FILE: '/run/t01/smtp-password',
    SMTP_FROM: 'no-reply@identity.example',
    SMTP_TLS: 'required',
  };
  const containerId = (await compose('ps', '-q', 'api')).stdout.trim();
  assert.match(containerId, /^[a-f0-9]{12,64}$/u, 'API container is missing; run npm run dev:up first.');
  const inspect = await run(['inspect', '--format', '{{.Image}}', containerId]);
  assert.equal(inspect.code, 0, 'Cannot inspect API image');
  const imageId = inspect.stdout.trim();
  assert.match(imageId, /^sha256:[a-f0-9]{64}$/u, 'API image is missing; run npm run dev:up first.');
  const localDirectory = resolve(root, '.local');
  const privateKey = await readFile(resolve(localDirectory, 'signing.pem'), 'utf8');
  const encryptionKeys = JSON.parse(await readFile(resolve(localDirectory, 'encryption-keys.json'), 'utf8'));
  const protectedValues = [
    baseline.DATABASE_URL, new URL(baseline.DATABASE_URL).password,
    baseline.SMTP_USERNAME, smtpPassword,
    ...privateKey.split('\n').filter((line) => line.length > 60 && !line.startsWith('---')),
    ...Object.values(encryptionKeys), 'never-log',
  ];
  const configRun = async (name, changes, expectedField) => {
    const values = { ...baseline, ...changes };
    const configPath = resolve(privateTestDir, 'case.env');
    await writeFile(configPath, `${Object.entries(values).map(([key, value]) => `${key}=${value}`).join('\n')}\n`, { mode: 0o600 });
    const result = await run(['run', '--rm', '--env-file', configPath,
      '--mount', `type=bind,src=${resolve(localDirectory, 'signing.pem')},dst=/run/secrets/signing.pem,readonly`,
      '--mount', `type=bind,src=${resolve(localDirectory, 'encryption-keys.json')},dst=/run/secrets/encryption-keys.json,readonly`,
      '--mount', `type=bind,src=${privateTestDir},dst=/run/t01,readonly`,
      imageId, 'identity-server', '--check-config']);
    const output = result.stdout + result.stderr;
    for (const secret of protectedValues) {
      assert.ok(!output.includes(secret), `${name}: sensitive configuration was logged`);
    }
    if (expectedField) {
      assert.notEqual(result.code, 0, `${name}: invalid configuration accepted`);
      assert.ok(output.includes(`invalid or missing configuration: ${expectedField}`), `${name}: error must identify configuration name only`);
    } else {
      assert.equal(result.code, 0, `${name}: valid production configuration rejected`);
    }
    lines.push(`PASS configuration ${name}: exit=${result.code}; field=${expectedField ?? 'valid'}`);
  };
  await configRun('production baseline', {}, null);
  for (const field of ['APP_ENV', 'DATABASE_URL', 'REDIS_URL', 'SIGNING_KEY_FILE', 'SIGNING_KID', 'ENCRYPTION_KEYS_FILE', 'ACTIVE_ENCRYPTION_KID', 'SMTP_USERNAME', 'SMTP_PASSWORD_FILE']) {
    await configRun(`missing ${field}`, { [field]: '' }, field);
  }
  for (const [name, changes, field] of [
    ['HTTP issuer', { ISSUER: 'http://identity.example' }, 'ISSUER'],
    ['issuer path', { ISSUER: 'https://identity.example/path' }, 'ISSUER'],
    ['issuer query', { ISSUER: 'https://identity.example?secret=never-log' }, 'ISSUER'],
    ['issuer userinfo', { ISSUER: 'https://user:never-log@identity.example' }, 'ISSUER'],
    ['normalized path', { ISSUER: 'https://identity.example/a/..' }, 'ISSUER'],
    ['issuer whitespace', { ISSUER: ' https://identity.example ' }, 'ISSUER'],
    ['wrong RP', { RP_ID: 'other.example' }, 'RP_ID'],
    ['missing key file', { SIGNING_KEY_FILE: '/run/t01/missing.pem' }, 'SIGNING_KEY_FILE'],
    ['invalid AEAD version', { ACTIVE_ENCRYPTION_KID: 'absent' }, 'ACTIVE_ENCRYPTION_KID'],
    ['plaintext SMTP', { SMTP_TLS: 'disabled' }, 'SMTP_TLS'],
    ['whitespace SMTP username', { SMTP_USERNAME: '   ' }, 'SMTP_USERNAME'],
    ['SMTP host credentials', { SMTP_HOST: 'user:never-log@host' }, 'SMTP_HOST'],
    ['invalid SMTP from', { SMTP_FROM: '"bad@example' }, 'SMTP_FROM'],
    ['TLS verification disabled', { SMTP_TLS_VERIFY: 'false' }, 'SMTP_TLS_VERIFY'],
    ['insecure cookie', { COOKIE_SECURE: 'false' }, 'COOKIE_SECURE'],
    ['script-readable cookie', { COOKIE_HTTP_ONLY: 'false' }, 'COOKIE_HTTP_ONLY'],
    ['cookie Domain', { COOKIE_DOMAIN: 'example' }, 'COOKIE_DOMAIN'],
    ['cookie path', { COOKIE_PATH: '/api' }, 'COOKIE_PATH'],
    ['cookie SameSite', { COOKIE_SAME_SITE: 'None' }, 'COOKIE_SAME_SITE'],
    ['development seed', { DEV_SEED: 'true' }, 'DEV_SEED'],
    ['debug routes', { DEBUG_ROUTES: 'true' }, 'DEBUG_ROUTES'],
    ['unbounded hash tasks', { ARGON2_PARALLELISM_LIMIT: '5' }, 'ARGON2_PARALLELISM_LIMIT'],
    ['invalid trusted proxy', { TRUSTED_PROXY_CIDRS: 'user-provided-host' }, 'TRUSTED_PROXY_CIDRS'],
  ]) await configRun(name, changes, field);
  await writeFile(resolve(privateTestDir, 'bad.pem'), 'invalid private key\n', { mode: 0o600 });
  await configRun('invalid private key', { SIGNING_KEY_FILE: '/run/t01/bad.pem' }, 'SIGNING_KEY_FILE');
  await writeFile(resolve(privateTestDir, 'bad-encryption.json'), '{"weak":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}\n', { mode: 0o600 });
  await configRun('all-zero AEAD key', { ENCRYPTION_KEYS_FILE: '/run/t01/bad-encryption.json', ACTIVE_ENCRYPTION_KID: 'weak' }, 'ENCRYPTION_KEYS_FILE');
  const key = Object.values(encryptionKeys)[0];
  await writeFile(resolve(privateTestDir, 'duplicate-encryption.json'), `{"duplicate":"${key}","duplicate":"${key}"}\n`, { mode: 0o600 });
  await configRun('duplicate AEAD key version', { ENCRYPTION_KEYS_FILE: '/run/t01/duplicate-encryption.json', ACTIVE_ENCRYPTION_KID: 'duplicate' }, 'ENCRYPTION_KEYS_FILE');
  for (const [name, jwks] of [
    ['invalid public material', { keys: [{ kty: 'RSA', alg: 'RS256', use: 'sig', kid: 'old', n: '!!!', e: '!!!' }] }],
    ['private JWKS field', { keys: [{ kty: 'RSA', alg: 'RS256', use: 'sig', kid: 'old', n: 'AQ', e: 'AQAB', d: 'never-log' }] }],
    ['duplicate signing kid', { keys: [{ kty: 'RSA', alg: 'RS256', use: 'sig', kid: 'local-signing-1', n: 'AQ', e: 'AQAB' }] }],
  ]) {
    await writeFile(resolve(privateTestDir, 'bad-jwks.json'), JSON.stringify(jwks), { mode: 0o600 });
    await configRun(name, { JWKS_PREVIOUS_FILE: '/run/t01/bad-jwks.json' }, 'JWKS_PREVIOUS_FILE');
  }
}

async function main() {
  // Docker Compose releases emit either a JSON array or one object per line.
  const raw = (await compose('ps', '--all', '--format', 'json')).stdout.trim();
  const states = raw.startsWith('[') ? JSON.parse(raw) : raw.split('\n').filter(Boolean).map((line) => JSON.parse(line));
  const required = ['postgres', 'redis', 'mailpit', 'api', 'worker', 'identity-web', 'demo-a', 'demo-b'];
  for (const name of required) {
    const service = states.find((state) => state.Service === name);
    assert.ok(service, `Missing Compose service: ${name}`);
    assert.equal(service.State, 'running', `${name} must be running`);
    assert.equal(service.Health, 'healthy', `${name} must be healthy`);
    lines.push(`PASS service ${name}: running/healthy`);
  }
  await health(200);
  lines.push('PASS initial live=200 / ready=200');
  for (const port of [5173, 5174, 5175]) {
    const response = await fetch(`http://localhost:${port}/`, { signal: AbortSignal.timeout(5000) });
    assert.equal(response.status, 200);
    assert.match(await response.text(), /T01/u);
    lines.push(`PASS frontend ${port}: HTTP 200 / T01 title`);
  }
  const proxy = await fetch('http://localhost:5173/health/ready', { signal: AbortSignal.timeout(5000) });
  assert.equal(proxy.status, 200);
  assert.deepEqual(await proxy.json(), { status: 'ready' });
  const mailpit = await fetch('http://localhost:8025/readyz', { signal: AbortSignal.timeout(5000) });
  assert.equal(mailpit.status, 200);
  lines.push('PASS same-origin Vite health proxy and Mailpit readiness');
  await configCases();
  for (const dependency of ['redis', 'postgres']) {
    await compose('stop', dependency);
    try {
      await health(503, 5);
      lines.push(`PASS ${dependency} stopped: live=200 / ready=503`);
    } finally {
      await compose('start', dependency);
    }
    await health(200);
    lines.push(`PASS ${dependency} restored: live=200 / ready=200`);
  }
  lines.push(`Completed: ${new Date().toISOString()}`, 'Exit code: 0');
  console.log('T01: real Compose startup, production configuration rejection, and dependency outage/recovery checks passed.');
}

let passed = false;
try {
  await main();
  passed = true;
} catch (error) {
  // Never persist child output or user-provided configuration.
  lines.push(`FAIL: ${error.message}`, 'Exit code: 1');
  console.error(error.message);
  process.exitCode = 1;
} finally {
  await mkdir(evidence, { recursive: true });
  await writeFile(resolve(evidence, passed ? 'integration.txt' : 'integration-failure.txt'), `${lines.join('\n')}\n`);
  await rm(privateTestDir, { recursive: true, force: true });
}
