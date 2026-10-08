// Full local identity snapshot recovery with isolated PostgreSQL and fresh browser ceremonies.
import assert from 'node:assert/strict';
import { createHash, randomBytes, randomUUID } from 'node:crypto';
import { execFileSync, spawn } from 'node:child_process';
import { chown, cp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import { resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { chromium } from 'playwright';
import { createLocalJWKSet, jwtVerify } from 'jose';
import { localEnvironment } from '../../scripts/database-target.mjs';
import { ceremonyClient } from './identity-ceremonies.mjs';

const root = resolve(import.meta.dirname, '../..');
const taskDirectory = resolve(root, `.local/identity-recovery-${randomBytes(8).toString('hex')}`);
const source = resolve(taskDirectory, 'source'); const recovered = resolve(taskDirectory, 'recovered');
const image = 'docker.m.daocloud.io/library/postgres:17.11-bookworm@sha256:3645570cccdfa447589da9f57dd740faa29b30938e861289a5574b6ca6b03826';
const age = process.env.AGE_BINARY ?? resolve(root, '.local/security-tools/age/age');
const keygen = process.env.AGE_KEYGEN_BINARY ?? resolve(root, '.local/security-tools/age/age-keygen');
const containers = [];
const report = { started: new Date().toISOString(), kind: 'Local full identity base snapshot recovery, virtual WebAuthn; not production RPO/RTO', postgresImage: image, checks: [], sourceSha256: {}, exitCode: 1 };
let harness; let browser; let context;
const privateValues = [];
function command(program, args) { return execFileSync(program, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }); }
function docker(...args) { return command('docker', args); }
function startHarness(env) {
  const child = spawn(process.env.T22_RESTORE_CARGO ?? 'cargo', ['test', '-p', 'identity-server', '--test', 't22_identity_restore', '--locked', '--', '--exact', 't22_identity_restore_harness', '--nocapture'], { cwd: root, env, stdio: ['pipe', 'pipe', 'pipe'] });
  let output = ''; child.stdout.on('data', (text) => { output += text; }); child.stderr.on('data', (text) => { output += text; });
  const completion = new Promise((done) => { child.on('error', () => done(127)); child.on('close', done); });
  return { child, completion, output: () => output };
}
async function stopHarness() {
  if (!harness) return;
  harness.child.stdin.end('stop\n'); const code = await harness.completion;
  await writeFile(resolve(taskDirectory, `harness-${Date.now()}.txt`), harness.output(), { mode: 0o600 });
  assert.equal(code, 0, 'The isolated API harness must close without skipping checks.'); harness = undefined;
}
async function readyHarness(env) {
  harness = startHarness(env);
  for (let attempt = 0; attempt < 600; attempt += 1) {
    if (harness.output().includes('T22_IDENTITY_RESTORE_READY')) return;
    if (harness.child.exitCode !== null) break;
    await delay(100);
  }
  throw new Error('Real restore API harness did not start.');
}
async function pg(directory, restored = false) {
  const name = `identity-recovery-${randomUUID()}`; containers.push(name);
  await mkdir(resolve(directory, 'socket'), { recursive: true, mode: 0o700 });
  await chown(resolve(directory, 'socket'), 999, 999);
  const args = ['run', '-d', '--network', 'none', '--name', name, '-v', `${directory}:/fixture`, '-v', `${resolve(directory, 'socket')}:/var/run/postgresql`];
  if (restored) args.push('-v', `${resolve(directory, 'pgdata')}:/var/lib/postgresql/data`);
  else args.push('-e', 'POSTGRES_HOST_AUTH_METHOD=trust', '-e', 'POSTGRES_DB=identity_test');
  args.push(image);
  docker(...args);
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try { if (docker('exec', name, 'pg_isready', '-h', '127.0.0.1', '-U', 'postgres').includes('accepting connections')) return name; } catch { /* Poll only owned container startup. */ }
    await delay(100);
  }
  throw new Error('Isolated recovery PostgreSQL did not become ready.');
}
async function protocol(issuer, client, secret, path, parameters) {
  const response = await fetch(`${issuer}${path}`, { method: 'POST', headers: { authorization: `Basic ${Buffer.from(`${client}:${secret}`).toString('base64')}`, 'content-type': 'application/x-www-form-urlencoded' }, body: new URLSearchParams(parameters) });
  const text = await response.text();
  return { status: response.status, body: text ? JSON.parse(text) : null };
}
async function grant(ceremony, issuer, client) {
  const verifier = randomBytes(32).toString('base64url');
  const parameters = new URLSearchParams({ client_id: client.client_id, response_type: 'code', redirect_uri: `${issuer}/callback`, scope: 'openid email', state: randomBytes(16).toString('base64url'), nonce: randomBytes(16).toString('base64url'), code_challenge: createHash('sha256').update(verifier).digest('base64url'), code_challenge_method: 'S256', prompt: 'consent' });
  // Node receives the real authorize redirect without browser fetch hiding an opaque redirect.
  const cookie = (await context.cookies(issuer)).map((item) => `${item.name}=${item.value}`).join('; ');
  const authorized = await fetch(`${issuer}/oauth/authorize?${parameters}`, { headers: { cookie }, redirect: 'manual' }); assert.equal(authorized.status, 302);
  const id = authorized.headers.get('location').split('/').at(-1);
  const approved = await ceremony.request(`/api/v1/oauth/transactions/${id}/decision`, { decision: 'approve' }); assert.equal(approved.status, 200);
  const callback = new URL(approved.body.redirect_to);
  const exchanged = await protocol(issuer, client.client_id, client.secret, '/oauth/token', { grant_type: 'authorization_code', code: callback.searchParams.get('code'), code_verifier: verifier, redirect_uri: `${issuer}/callback` }); assert.equal(exchanged.status, 200);
  privateValues.push(exchanged.body.access_token, exchanged.body.refresh_token, exchanged.body.id_token);
  return exchanged.body;
}
try {
  await mkdir(source, { recursive: true, mode: 0o700 }); await mkdir(recovered, { recursive: true, mode: 0o700 });
  assert.match(command(age, ['--version']), /1\.3\.2/u);
  const password = `Recovery ceremony ${randomBytes(32).toString('base64url')}`; const control = randomBytes(32).toString('base64url'); const aead = randomBytes(32).toString('base64'); const active = `recovery-${randomBytes(8).toString('hex')}`;
  privateValues.push(password, control, aead);
  command('openssl', ['genpkey', '-algorithm', 'RSA', '-pkeyopt', 'rsa_keygen_bits:2048', '-out', resolve(source, 'signing.pem')]);
  await writeFile(resolve(source, 'keys.json'), JSON.stringify({ [active]: aead }), { mode: 0o600 });
  command(keygen, ['-o', resolve(source, 'backup.agekey')]); const recipient = command(keygen, ['-y', resolve(source, 'backup.agekey')]).trim();
  for (const file of ['signing.pem', 'backup.agekey']) privateValues.push(await readFile(resolve(source, file), 'utf8'));
  const server = createServer(); await new Promise((done) => server.listen(0, '127.0.0.1', done)); const port = server.address().port; await new Promise((done) => server.close(done));
  const issuer = `http://localhost:${port}`;
  const primary = await pg(source);
  const local = await localEnvironment(root); const redis = new URL(local.REDIS_URL); redis.hostname = '127.0.0.1'; privateValues.push(redis.toString());
  const environment = (directory, mode) => ({ ...process.env, APP_ENV: 'test', TEST_DATABASE_URL: `postgres://postgres@localhost/identity_test?host=${encodeURIComponent(resolve(directory, 'socket'))}`, REDIS_URL: redis.toString(), T22_RESTORE_DIRECTORY: directory, T22_RESTORE_ISSUER: issuer, T22_RESTORE_MODE: mode, T22_RESTORE_SIGNING_FILE: resolve(directory, 'signing.pem'), T22_RESTORE_KEYS_FILE: resolve(directory, 'keys.json'), T22_RESTORE_ACTIVE_KID: active, T22_RESTORE_PASSWORD: password, T22_RESTORE_CONTROL_KEY: control });
  await readyHarness(environment(source, 'primary'));
  browser = await chromium.launch(); context = await browser.newContext();
  const ceremony = await ceremonyClient(context, issuer, control);
  const cdp = await context.newCDPSession(ceremony.page); await cdp.send('WebAuthn.enable'); const certificate = await cdp.send('WebAuthn.addVirtualAuthenticator', { options: { protocol: 'ctap2', transport: 'internal', hasResidentKey: true, hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true } });
  const scenario = JSON.parse(await readFile(resolve(source, 'scenario.json'), 'utf8')); for (const client of scenario.clients) privateValues.push(client.secret);
  await ceremony.password(scenario.email, password);
  assert.equal((await ceremony.request('/api/v1/me/reauth/password', { password })).status, 200);
  await ceremony.passkey('create');
  const strong = await ceremony.passkey('reauth'); assert.equal(strong.strong, true);
  const exported = await cdp.send('WebAuthn.getCredentials', { authenticatorId: certificate.authenticatorId }); assert.equal(exported.credentials.length, 1);
  privateValues.push(exported.credentials[0].privateKey);
  await cdp.send('WebAuthn.removeVirtualAuthenticator', { authenticatorId: certificate.authenticatorId }); await cdp.detach();
  const factor = await ceremony.enroll(); privateValues.push(factor);
  assert.equal((await ceremony.request('/api/v1/auth/logout', {})).status, 204);
  await ceremony.close();
  const before = await ceremonyClient(context, issuer, control); await before.password(scenario.email, password, factor);
  const live = await grant(before, issuer, scenario.clients[0]); const revoked = await grant(before, issuer, scenario.clients[1]);
  assert.equal((await protocol(issuer, scenario.clients[1].client_id, scenario.clients[1].secret, '/oauth/revoke', { token: revoked.refresh_token })).status, 200);
  for (const [index, bundle, expected] of [[0, live, true], [1, revoked, false]]) { const checked = await protocol(issuer, scenario.clients[index].client_id, scenario.clients[index].secret, '/oauth/introspect', { token: bundle.access_token }); assert.equal(checked.status, 200); assert.equal(checked.body.active, expected); }
  report.checks.push('Before backup: real password/TOTP and virtual Passkey enrollment, live OAuth family, and separately revoked OAuth family.');
  await stopHarness();
  // Real physical PG snapshot, encrypted with public recipient; snapshot private keys travel separately.
  docker('exec', '-e', 'BACKUP_DESTINATION=/fixture/backup', '-e', `AGE_RECIPIENT=${recipient}`, '-e', 'AGE_BINARY=/opt/age', '-e', 'PGPASSFILE=/fixture/pgpass', '-e', 'PGHOST=/var/run/postgresql', '-e', 'PGUSER=postgres', primary, 'sh', '-c', 'touch /fixture/pgpass && chmod 600 /fixture/pgpass');
  docker('cp', age, `${primary}:/opt/age`); docker('exec', primary, 'chmod', '755', '/opt/age'); docker('cp', resolve(root, 'infra/ops/base-backup.sh'), `${primary}:/opt/base-backup.sh`);
  docker('exec', '-e', 'BACKUP_DESTINATION=/fixture/backup', '-e', `AGE_RECIPIENT=${recipient}`, '-e', 'AGE_BINARY=/opt/age', '-e', 'PGPASSFILE=/fixture/pgpass', '-e', 'PGHOST=/var/run/postgresql', '-e', 'PGUSER=postgres', primary, 'sh', '/opt/base-backup.sh');
  const backupName = (await readdir(resolve(source, 'backup/base'))).find((file) => file.endsWith('.tar.age')); assert.ok(backupName);
  for (const file of ['signing.pem', 'keys.json', 'backup.agekey', 'scenario.json', 'clock.json']) await cp(resolve(source, file), resolve(recovered, file));
  await cp(resolve(source, 'backup/base'), resolve(recovered, 'backup'), { recursive: true });
  for (const file of ['signing.pem', 'keys.json']) assert.equal(createHash('sha256').update(await readFile(resolve(source, file))).digest('hex'), createHash('sha256').update(await readFile(resolve(recovered, file))).digest('hex'));
  report.backupId = backupName.replace(/\.tar\.age$/u, ''); report.encryptedBackupSha256 = createHash('sha256').update(await readFile(resolve(recovered, 'backup', backupName))).digest('hex');
  // Primary is our own task container; remove it before restoration to prove no reads of its DB.
  docker('rm', '-f', '-v', primary); containers.splice(containers.indexOf(primary), 1);
  await rm(source, { recursive: true, force: true });
  const restoreStart = performance.now();
  const utility = `identity-recovery-utility-${randomUUID()}`; containers.push(utility);
  docker('run', '-d', '--network', 'none', '--name', utility, '--entrypoint', 'sleep', '-v', `${recovered}:/fixture`, '-v', `${resolve(root, 'infra/ops/restore-base.sh')}:/opt/restore-base.sh:ro`, '-v', `${age}:/opt/age:ro`, image, 'infinity');
  docker('exec', '-e', 'AGE_IDENTITY_FILE=/fixture/backup.agekey', '-e', 'AGE_BINARY=/opt/age', utility, 'sh', '/opt/restore-base.sh', `/fixture/backup/${backupName}`, '/fixture/pgdata');
  docker('exec', utility, 'chown', '-R', '999:999', '/fixture/pgdata');
  docker('rm', '-f', '-v', utility); containers.splice(containers.indexOf(utility), 1);
  await pg(recovered, true); await readyHarness(environment(recovered, 'recovered'));
  const restored = await ceremonyClient(context, issuer, control);
  assert.equal((await restored.request('/api/v1/me')).status, 200, 'Backed-up active session must survive recovery.');
  for (const [index, bundle, expected] of [[0, live, true], [1, revoked, false]]) { const checked = await protocol(issuer, scenario.clients[index].client_id, scenario.clients[index].secret, '/oauth/introspect', { token: bundle.access_token }); assert.equal(checked.status, 200); assert.equal(checked.body.active, expected); }
  const refreshed = await protocol(issuer, scenario.clients[0].client_id, scenario.clients[0].secret, '/oauth/token', { grant_type: 'refresh_token', refresh_token: live.refresh_token }); assert.equal(refreshed.status, 200); privateValues.push(refreshed.body.access_token, refreshed.body.refresh_token, refreshed.body.id_token);
  const header = JSON.parse(Buffer.from(refreshed.body.id_token.split('.')[0], 'base64url').toString()); assert.equal(header.kid, 'restore-signing');
  const jwtPayload = JSON.parse(Buffer.from(refreshed.body.id_token.split('.')[1], 'base64url').toString());
  const jwks = await (await fetch(`${issuer}/oauth/jwks`)).json();
  const verified = await jwtVerify(refreshed.body.id_token, createLocalJWKSet(jwks), { algorithms: ['RS256'], issuer, audience: scenario.clients[0].client_id, currentDate: new Date(Number(jwtPayload.iat) * 1000) });
  assert.equal(verified.payload.sub, scenario.user_id);
  assert.equal((await protocol(issuer, scenario.clients[1].client_id, scenario.clients[1].secret, '/oauth/token', { grant_type: 'refresh_token', refresh_token: revoked.refresh_token })).body.error, 'invalid_grant');
  assert.equal((await restored.request('/api/v1/auth/logout', {})).status, 204); await restored.close();
  const totpLogin = await ceremonyClient(context, issuer, control); await totpLogin.password(scenario.email, password, factor); assert.equal((await totpLogin.request('/api/v1/me')).status, 200); assert.equal((await totpLogin.request('/api/v1/auth/logout', {})).status, 204); await totpLogin.close();
  const passkeyLogin = await ceremonyClient(context, issuer, control);
  // Re-enable the same virtual authenticator in this new page's CDP session, preserving its key.
  const freshCdp = await context.newCDPSession(passkeyLogin.page); await freshCdp.send('WebAuthn.enable'); const freshAuth = await freshCdp.send('WebAuthn.addVirtualAuthenticator', { options: { protocol: 'ctap2', transport: 'internal', hasResidentKey: true, hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true } });
  await freshCdp.send('WebAuthn.addCredential', { authenticatorId: freshAuth.authenticatorId, credential: exported.credentials[0] });
  const assertion = await passkeyLogin.passkey('get'); assert.equal(assertion.status, 'authenticated'); assert.equal(assertion.user.id, scenario.user_id); assert.equal(assertion.session.amr.includes('user'), true); assert.equal(assertion.session.amr.includes('hwk'), false); assert.ok(assertion.session.strong_at); assert.equal((await passkeyLogin.request('/api/v1/me')).status, 200);
  const postRestore = await grant(passkeyLogin, issuer, scenario.clients[0]);
  assert.equal((await protocol(issuer, scenario.clients[0].client_id, scenario.clients[0].secret, '/oauth/introspect', { token: postRestore.access_token })).body.active, true);
  assert.equal((await passkeyLogin.request('/api/v1/auth/logout-all', {})).status, 204);
  assert.equal((await protocol(issuer, scenario.clients[0].client_id, scenario.clients[0].secret, '/oauth/introspect', { token: postRestore.access_token })).body.active, false);
  report.localRestoreSeconds = (performance.now() - restoreStart) / 1000;
  report.checks.push('Recovered different-directory database and separately copied signing/AEAD keys: active session, live access/refresh and revoked family preserved.');
  report.checks.push('Primary data/key source directory removed before recovery; new RS256 token validates with restored public JWKS, issuer/audience/sub.');
  report.checks.push('Recovered original password plus original TOTP completes a new step; private virtual credential transferred to a fresh CDP authenticator signs a new challenge for the same user, amr=user with strong_at.');
  report.checks.push('After recovery, real logout-all immediately makes refreshed OAuth access inactive.');
  await freshCdp.send('WebAuthn.removeVirtualAuthenticator', { authenticatorId: freshAuth.authenticatorId }); await freshCdp.detach();
  await passkeyLogin.close(); await stopHarness();
  report.commit = command('git', ['-C', root, 'rev-parse', 'HEAD']).trim();
  for (const file of ['crates/identity-server/tests/t22_identity_restore.rs', 'tests/ops/identity-restore.mjs', 'tests/ops/identity-ceremonies.mjs', 'infra/ops/base-backup.sh', 'infra/ops/restore-base.sh', 'Cargo.lock']) report.sourceSha256[file] = createHash('sha256').update(await readFile(resolve(root, file))).digest('hex');
  report.exitCode = 0; console.log('T22 isolated full identity recovery passed: password/TOTP/real virtual Passkey, live/revoked OAuth and post-restore logout.');
} catch (error) {
  report.failure = error.message; process.exitCode = 1;
  let diagnostic = `${error.stack ?? error}\n${harness?.output() ?? ''}`;
  for (const secret of privateValues) diagnostic = diagnostic.replaceAll(secret, '[REDACTED]');
  diagnostic = diagnostic.replace(/\b[A-Za-z0-9_-]{43,}\b/gu, '[OPAQUE]').replace(/-----BEGIN[^-]+-----[\s\S]*?-----END[^-]+-----/gu, '[KEY]');
  await mkdir(resolve(root, 'docs/evidence/T22'), { recursive: true });
  await writeFile(resolve(root, `docs/evidence/T22/identity-restore-failure-${Date.now()}.txt`), diagnostic);
  console.error('Local identity recovery failed; sanitized evidence retained, no success or skip claimed.');
} finally {
  if (harness) { harness.child.stdin?.end('stop\n'); await harness.completion; }
  await context?.close(); await browser?.close();
  for (const name of containers) { try { docker('rm', '-f', '-v', name); } catch { report.cleanupFailure = true; report.exitCode = 1; process.exitCode = 1; } }
  await rm(taskDirectory, { recursive: true, force: true });
  report.completed = new Date().toISOString();
  await mkdir(resolve(root, 'docs/evidence/T22'), { recursive: true });
  await writeFile(resolve(root, `docs/evidence/T22/identity-restore-${report.exitCode === 0 ? 'passed' : 'failure'}-${Date.now()}.json`), JSON.stringify(report, null, 2) + '\n');
}
