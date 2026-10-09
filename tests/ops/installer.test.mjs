import assert from 'node:assert/strict';
import test from 'node:test';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtemp, mkdir, writeFile, rm, stat, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { randomUUID } from 'node:crypto';
import { setTimeout as delay } from 'node:timers/promises';
import { validDomain, envText } from '../../infra/ops/install-production.mjs';
import { ApiSession } from '../../infra/ops/install-production.mjs';
import { createServer } from 'node:http';
import { backup } from '../../infra/ops/installer-operations.mjs';
import { verifiedBackupReceipt } from '../../infra/ops/collect-metrics.mjs';

const root = resolve(import.meta.dirname, '../..');
test('wizard accepts exact hostnames and refuses raw IP, URL, wildcards and configuration injection', () => {
  assert.equal(validDomain('auth.cdngod.com'), true);
  for (const value of ['111.10.137.17', 'https://auth.cdngod.com', '*.cdngod.com', 'auth.cdngod.com/path', 'auth\n.example', '-auth.example']) assert.equal(validDomain(value), false);
  assert.equal(envText({ APP_ENV: 'production', ISSUER: 'https://auth.cdngod.com' }), 'APP_ENV=production\nISSUER=https://auth.cdngod.com\n');
  for (const value of ['x\nAPP_ENV=test', 'x$(env)', 'x#comment', "x'quote", 'x\\escaped']) assert.throws(() => envText({ SMTP_HOST: value }));
});
test('single download shell parses and rejects bad release before installing or writing system files', () => {
  execFileSync('bash', ['-n', join(root, 'install.sh')]);
  assert.match(execFileSync('bash', [join(root, 'install.sh'), '--help'], { encoding: 'utf8' }), /never runs cargo\/npm\/docker build/u);
  const invalid = spawnSync('bash', [join(root, 'install.sh'), '--release', '../latest'], { encoding: 'utf8' }); assert.equal(invalid.status, 2);
});
test('installer session rotates CSRF across MFA and removes expired cookie values', async (t) => {
  let step = 0;
  const server = createServer((request, response) => {
    step++;
    if (step === 1) { response.setHeader('Set-Cookie', 'preauth=first; HttpOnly'); response.end(JSON.stringify({ csrf_token: 'first-csrf' })); }
    else if (step === 2) { assert.equal(request.headers.origin, `http://127.0.0.1:${server.address().port}`); assert.equal(request.headers['x-csrf-token'], 'first-csrf'); response.setHeader('Set-Cookie', ['preauth=; Max-Age=0', 'preauth=second; HttpOnly']); response.end(JSON.stringify({ status: 'mfa_required' })); }
    else if (step === 3) { assert.equal(request.headers.cookie, 'preauth=second'); response.end(JSON.stringify({ csrf_token: 'second-csrf' })); }
    else { assert.equal(request.headers['x-csrf-token'], 'second-csrf'); assert.equal(request.headers.cookie, 'preauth=second'); response.setHeader('Set-Cookie', ['preauth=; Max-Age=0', 'session=active; HttpOnly']); response.end(JSON.stringify({ status: 'authenticated', csrf_token: 'logged-csrf' })); }
  });
  await new Promise((done) => server.listen(0, '127.0.0.1', done)); t.after(() => new Promise((done) => server.close(done)));
  const session = new ApiSession(`http://127.0.0.1:${server.address().port}`); await session.request('/login', { password: 'fixture' }); await session.request('/factor', { code: 'fixture' }); assert.equal(step, 4); assert.equal(session.cookies.get('session'), 'active'); assert.equal(session.cookies.has('preauth'), false); assert.equal(session.csrf, 'logged-csrf');
});
test('installer backup uses actual isolated PostgreSQL, verifies then encrypts, and missing device cannot publish', async (t) => {
  const directory = await mkdtemp(join(tmpdir(), 'identity-installer-')); const container = `installer-backup-${randomUUID()}`;
  t.after(async () => { spawnSync('docker', ['rm', '-f', '-v', container], { stdio: 'ignore' }); await rm(directory, { recursive: true, force: true }); });
  const backupRoot = join(directory, 'backups'); await mkdir(join(directory, '.local/production'), { recursive: true, mode: 0o700 }); await mkdir(join(directory, 'infra')); await mkdir(backupRoot); await mkdir(join(backupRoot, 'wal')); await mkdir(join(backupRoot, 'base'));
  const age = process.env.AGE_BINARY ?? join(root, '.local/security-tools/age/age'); const keygen = process.env.AGE_KEYGEN_BINARY ?? join(root, '.local/security-tools/age/age-keygen'); const key = join(directory, 'age.key'); execFileSync(keygen, ['-o', key], { stdio: ['ignore', 'ignore', 'pipe'] }); const recipient = execFileSync(keygen, ['-y', key], { encoding: 'utf8' }).trim();
  const settings = { project: 'installer-actual-backup', backupRoot, backupDevice: String((await stat(backupRoot, { bigint: true })).dev) }; await writeFile(join(directory, '.local/production/installer.json'), JSON.stringify(settings), { mode: 0o600 });
  execFileSync('docker', ['run', '-d', '--network', 'none', '--name', container, '-e', 'POSTGRES_USER=identity', '-e', 'POSTGRES_HOST_AUTH_METHOD=trust', '-e', `AGE_RECIPIENT=${recipient}`, '-v', `${resolve(age)}:/opt/identity-ops/age:ro`, 'docker.m.daocloud.io/library/postgres:17.11-bookworm@sha256:3645570cccdfa447589da9f57dd740faa29b30938e861289a5574b6ca6b03826'], { stdio: 'pipe' });
  let ready = false; for (let n = 0; n < 100; n++) { if (spawnSync('docker', ['exec', container, 'pg_isready', '-h', '127.0.0.1', '-U', 'identity'], { stdio: 'ignore' }).status === 0) { ready = true; break; } await delay(100); } assert.ok(ready);
  execFileSync('docker', ['exec', container, 'psql', '-U', 'identity', '-c', 'CREATE TABLE installer_probe(id integer); INSERT INTO installer_probe VALUES(1);'], { stdio: 'pipe' });
  const id = execFileSync('docker', ['inspect', '-f', '{{.Id}}', container], { encoding: 'utf8' }).trim();
  await backup(directory, async () => ({ code: 0, output: id })); const receipt = await verifiedBackupReceipt(join(backupRoot, 'base'), Math.floor(Date.now() / 1000)); assert.ok(receipt.ciphertext_bytes > 0);
  const tar = join(directory, 'base.tar'); execFileSync(age, ['-d', '-i', key, '-o', tar, join(backupRoot, 'base', receipt.backup_name)], { stdio: 'pipe' });
  assert.match(execFileSync('tar', ['-tf', tar], { encoding: 'utf8' }), /base\/backup_manifest/u);
  const original = await readFile(join(backupRoot, 'base/last-success.json'), 'utf8'); settings.backupDevice = '0'; await writeFile(join(directory, '.local/production/installer.json'), JSON.stringify(settings)); await assert.rejects(backup(directory), /mount/u); assert.equal(await readFile(join(backupRoot, 'base/last-success.json'), 'utf8'), original);
});
