// Interactive, dependency-free installation of prebuilt production products.
import { createInterface } from 'node:readline/promises';
import { stdin, stdout } from 'node:process';
import { mkdir, readFile, writeFile, stat, chown, chmod, access, readdir, rm } from 'node:fs/promises';
import { randomBytes, generateKeyPairSync } from 'node:crypto';
import { resolve, join } from 'node:path';
import { lookup } from 'node:dns/promises';
import { createServer } from 'node:net';
import { validateBundle } from './deploy-production.mjs';
import { executeCommand, configuration, runRelease, databaseFacts } from './release.mjs';

let nginxStoppedByInstaller = false;

export function validDomain(value) { return typeof value === 'string' && value.length <= 253 && value.includes('.') && !/^\d+(\.\d+){3}$/u.test(value) && value.split('.').every((part) => /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/u.test(part)); }
export function envText(values) { return Object.entries(values).map(([key, value]) => { if (!/^[A-Z][A-Z0-9_]*$/u.test(key) || /[\r\n\0]/u.test(String(value))) throw new Error('Invalid configuration text.'); const text = String(value); if (/[\s'"$\\#]/u.test(text)) throw new Error(`Unsupported special characters in ${key}; use a file-based secret.`); return `${key}=${text}`; }).join('\n') + '\n'; }
async function hidden(prompt) {
  stdout.write(prompt); stdin.setRawMode(true); stdin.resume(); let value = '';
  return new Promise((done, fail) => { const onData = (bytes) => { for (const character of bytes.toString()) { if (character === '\u0003') { finish(); fail(new Error('Cancelled.')); return; } if (character === '\r' || character === '\n') { finish(); done(value); return; } if (character === '\u007f' || character === '\b') value = Array.from(value).slice(0, -1).join(''); else if (character >= ' ') value += character; } }; const finish = () => { stdin.off('data', onData); stdin.setRawMode(false); stdin.pause(); stdout.write('\n'); }; stdin.on('data', onData); });
}
async function question(text, defaultValue = '') { const input = createInterface({ input: stdin, output: stdout }); try { return (await input.question(`${text}${defaultValue ? ` [${defaultValue}]` : ''}: `)).trim() || defaultValue; } finally { input.close(); } }
async function required(text, validator, defaultValue = '') { for (;;) { const value = await question(text, defaultValue); if (validator(value)) return value; console.log('Invalid value; please retry.'); } }
async function secretFile(path, bytes, uid = 10001) { await writeFile(path, bytes, { flag: 'wx', mode: 0o600 }); await chown(path, uid, uid); }
export async function detectProxy(executor = executeCommand) {
  const version = await executor('nginx', ['-v']);
  const active = await executor('systemctl', ['is-active', '--quiet', 'nginx']);
  return { nginxInstalled: version.code === 0, nginxServiceActive: active.code === 0 };
}
export async function portAvailable(port) {
  return new Promise((done, fail) => {
    const server = createServer();
    server.once('error', (error) => { if (error.code === 'EADDRINUSE' || error.code === 'EACCES') done(false); else fail(error); });
    server.listen({ port, host: '0.0.0.0', exclusive: true }, () => server.close(() => done(true)));
  });
}
export async function prepareProxy({ executor = executeCommand, ask = question, available = portAvailable } = {}) {
  const detected = await detectProxy(executor);
  if (detected.nginxServiceActive) {
    console.log('Nginx is running. This stack uses the prebuilt Docker Caddy for HTTPS and automatic certificates.');
    const answer = await ask('Stop Nginx and switch these web ports to Docker Caddy? Existing Nginx sites will stop. y/N', 'N');
    if (answer.toLowerCase() !== 'y') throw new Error('Nginx retained. Installation stopped without changing Nginx configuration.');
    const stopped = await executor('systemctl', ['stop', 'nginx']); if (stopped.code) throw new Error('Could not stop Nginx.');
    nginxStoppedByInstaller = true;
  } else {
    console.log(detected.nginxInstalled ? 'Nginx is installed but inactive. Using the CI-built Docker Caddy.' : 'No Nginx service found. Installing Caddy from the CI-built Docker image.');
  }
  try {
    for (const port of [80, 443]) if (!await available(port)) throw new Error(`Host port ${port} is already occupied. Resolve the conflict before installing Caddy.`);
  } catch (error) {
    if (detected.nginxServiceActive) { await executor('systemctl', ['start', 'nginx']); nginxStoppedByInstaller = false; }
    throw error;
  }
  return { ...detected, proxy: 'docker-caddy', nginxStoppedByInstaller: detected.nginxServiceActive };
}
async function install(root) {
  if (!stdin.isTTY || !stdout.isTTY || process.getuid?.() !== 0) throw new Error('Root interactive terminal required.');
  const manifest = await validateBundle(root); const local = join(root, '.local/production'); const wizardPath = join(local, 'installer.json');
  let exists = false; try { await access(wizardPath); exists = true; } catch (error) { if (error.code !== 'ENOENT') throw error; }
  if (exists) { try { await access(join(local, 'installed.json')); } catch { throw new Error('A partial installation is retained. Complete administrator/client setup from retained diagnostics before upgrading; no secrets will be regenerated.'); } console.log('Existing installation detected: keeping configuration, keys, administrator and database.'); const reply = await question(`Upgrade to ${manifest.revision}? y/N`, 'N'); if (reply.toLowerCase() !== 'y') return; const result = await executeCommand(process.execPath, [join(root, 'infra/ops/deploy-production.mjs'), 'deploy', join(local, 'deploy.json'), '--allow-production']); if (result.code) throw new Error('Upgrade failed; inspect controlled release-state diagnostics.'); console.log('Upgrade complete.'); return; }
  await mkdir(local, { recursive: true, mode: 0o700 }); await chmod(local, 0o700); const secrets = join(local, 'secrets'); await mkdir(secrets, { mode: 0o700 });
  if ((await readdir(secrets)).length) throw new Error('Partial installation secrets already exist; refusing to overwrite. Inspect the retained state before retrying.');
  const identity = await required('Identity domain (DNS must already point to this server)', validDomain);
  const appA = await required('Demo A domain', (v) => validDomain(v) && v !== identity);
  const appB = await required('Demo B domain', (v) => validDomain(v) && ![identity, appA].includes(v));
  for (const domain of [identity, appA, appB]) await lookup(domain);
  const tlsEmail = await required('TLS certificate contact email', (v) => /^[^\s@]+@[^\s@]+\.[^\s@]+$/u.test(v));
  const smtpHost = await required('SMTP host (STARTTLS with trusted certificate)', validDomain);
  const smtpPort = await required('SMTP port', (v) => /^\d+$/u.test(v) && Number(v) > 0 && Number(v) <= 65535, '587');
  const smtpUser = await required('SMTP username', (v) => v.length > 0 && !/[\s'"$\\#]/u.test(v));
  const smtpFrom = await required('SMTP sender email', (v) => /^[^\s@]+@[^\s@]+\.[^\s@]+$/u.test(v));
  const smtpPassword = await hidden('SMTP password (hidden): '); if (!smtpPassword || /[\r\n\0]/u.test(smtpPassword)) throw new Error('Invalid SMTP password.');
  const backupMount = await required('Existing independent backup mount', (v) => /^\/[a-zA-Z0-9_./-]+$/u.test(v) && !v.includes('/../') && v !== '/');
  const mountResult = await executeCommand('mountpoint', ['-q', backupMount]); if (mountResult.code) throw new Error('Backup location must already be mounted; no fallback directory will be created.');
  const backupRoot = join(backupMount, 'auth-rust'); await mkdir(backupRoot, { recursive: true, mode: 0o755 }); await mkdir(join(backupRoot, 'wal'), { mode: 0o700 }); await chown(join(backupRoot, 'wal'), 999, 999); await chmod(join(backupRoot, 'wal'), 0o700); await mkdir(join(backupRoot, 'base'), { mode: 0o700 });
  const device = String((await stat(join(backupRoot, 'wal'), { bigint: true })).dev);
  const recipient = await required('Age backup PUBLIC recipient (keep private key separately)', (v) => /^age1[0-9a-z]{58}$/u.test(v));
  const adminEmail = await required('First administrator email', (v) => /^[^\s@]+@[^\s@]+\.[^\s@]+$/u.test(v));
  const adminPassword = await hidden('First administrator password (15-128 characters, hidden): '); if (Array.from(adminPassword).length < 15 || Array.from(adminPassword).length > 128 || Buffer.byteLength(adminPassword) > 512) throw new Error('Invalid administrator password length.');
  if (await hidden('Repeat administrator password: ') !== adminPassword) throw new Error('Administrator passwords do not match.');
  const proxy = await prepareProxy();
  const postgresPassword = randomBytes(32).toString('hex'); const database = `postgres://identity:${postgresPassword}@postgres:5432/identity_production`;
  const key = generateKeyPairSync('rsa', { modulusLength: 3072, privateKeyEncoding: { type: 'pkcs8', format: 'pem' }, publicKeyEncoding: { type: 'spki', format: 'pem' } }).privateKey;
  await secretFile(join(secrets, 'signing.pem'), key); await secretFile(join(secrets, 'encryption-keys.json'), JSON.stringify({ 'production-aead-1': randomBytes(32).toString('base64') }) + '\n');
  await secretFile(join(secrets, 'smtp-password'), smtpPassword); await secretFile(join(secrets, 'postgres-password'), postgresPassword, 999); await secretFile(join(secrets, 'metrics-token'), randomBytes(32).toString('base64url')); await secretFile(join(secrets, 'jwks-previous.json'), '{"keys":[]}\n');
  for (const name of ['demo-a-secret', 'demo-b-secret']) await secretFile(join(secrets, name), randomBytes(32).toString('base64url'));
  const issuer = `https://${identity}`;
  const settings = { APP_ENV: 'production', BIND: '0.0.0.0:8080', ISSUER: issuer, RP_ID: identity, DATABASE_URL: database, REDIS_URL: 'redis://redis:6379', SIGNING_KEY_FILE: '/run/secrets/signing.pem', SIGNING_KID: 'production-signing-1', ENCRYPTION_KEYS_FILE: '/run/secrets/encryption-keys.json', ACTIVE_ENCRYPTION_KID: 'production-aead-1', SMTP_HOST: smtpHost, SMTP_PORT: smtpPort, SMTP_FROM: smtpFrom, SMTP_TLS: 'required', SMTP_USERNAME: smtpUser, SMTP_PASSWORD_FILE: '/run/secrets/smtp-password', JWKS_PREVIOUS_FILE: '/run/secrets/jwks-previous.json', METRICS_TOKEN_FILE: '/run/secrets/metrics-token' };
  await writeFile(join(local, 'identity.env'), envText(settings), { flag: 'wx', mode: 0o600 });
  for (const [name, domain, port] of [['a', appA, 8082], ['b', appB, 8083]]) await writeFile(join(local, `demo-${name}.env`), envText({ APP_ENV: 'production', BIND: `0.0.0.0:${port}`, BFF_PUBLIC_ORIGIN: `https://${domain}`, ISSUER: issuer, BFF_CLIENT_ID: `installer-demo-${name}`, BFF_CLIENT_SECRET_FILE: '/run/secrets/client-secret', DATABASE_URL: database, BFF_NAMESPACE: `demo_${name}`, BFF_COOKIE_NAME: `__Host-demo-${name}`, ENCRYPTION_KEYS_FILE: '/run/secrets/encryption-keys.json', ACTIVE_ENCRYPTION_KID: 'production-aead-1' }), { flag: 'wx', mode: 0o600 });
  const envFile = join(root, 'infra/production.env'); await writeFile(envFile, envText({ RUST_IMAGE: manifest.images.runtime, EDGE_IMAGE: manifest.images.edge, IDENTITY_HOST: identity, DEMO_A_HOST: appA, DEMO_B_HOST: appB, TLS_EMAIL: tlsEmail, BACKUP_DESTINATION: backupRoot, WAL_ARCHIVE_DEVICE: device, AGE_RECIPIENT: recipient }), { flag: 'wx', mode: 0o600 });
  const wizard = { version: 1, root, project: 'auth-rust-production', identity, appA, appB, issuer, adminEmail, proxy, backupMount, backupRoot, backupDevice: device, recipient };
  await writeFile(wizardPath, JSON.stringify(wizard, null, 2) + '\n', { flag: 'wx', mode: 0o600 });
  const operations = join(root, 'infra/ops/installer-operations.mjs'); const deployConfig = { project: wizard.project, envFile, stateDirectory: join(local, 'release-state'), backupDirectory: join(backupRoot, 'base'), databaseUser: 'identity', databaseName: 'identity_production', encryptionKeysFile: join(secrets, 'encryption-keys.json'), backup: { program: process.execPath, args: [operations, 'backup', root] }, smoke: { program: process.execPath, args: [operations, 'smoke', root] } };
  await writeFile(join(local, 'deploy.json'), JSON.stringify(deployConfig, null, 2) + '\n', { flag: 'wx', mode: 0o600 });
  const env = { RUST_IMAGE: manifest.images.runtime, EDGE_IMAGE: manifest.images.edge }; const compose = (args) => ['compose', '--project-name', wizard.project, '--env-file', envFile, '-f', join(root, 'infra/compose.prod.yaml'), ...args];
  const run = async (args, input) => { const result = await executeCommand('docker', args, { env, input }); await writeFile(join(local, 'installation-stage.txt'), result.output, { mode: 0o600 }); if (result.code) throw new Error('Installation stage failed; inspect owner-only installation-stage.txt. Configuration and data are retained.'); return result; };
  console.log('Pulling completed CI products; no compilation on this server.');
  for (const image of [...Object.values(manifest.images), 'docker.m.daocloud.io/library/postgres:17.11-bookworm@sha256:3645570cccdfa447589da9f57dd740faa29b30938e861289a5574b6ca6b03826', 'docker.m.daocloud.io/library/redis:7.4@sha256:4fa24486b8bcca8eec45ee0eb166edc674795e53a2b53d1a9ef263eecebaac85']) await run(['pull', image]);
  await run(compose(['config', '--quiet']));
  for (const [service, binary] of [['api', 'identity-server'], ['worker', 'identity-worker'], ['demo-a-bff', 'demo-bff'], ['demo-b-bff', 'demo-bff']]) await run(compose(['run', '--rm', '--no-deps', '--no-build', service, binary, '--check-config']));
  await run(compose(['up', '-d', '--no-build', '--wait', '--wait-timeout', '120', 'postgres', 'redis']));
  console.log('Backing up the initial database before migration.');
  const backup = await executeCommand(process.execPath, [operations, 'backup', root]); if (backup.code) throw new Error('Initial backup failed; migration not run.');
  await run(compose(['--profile', 'maintenance', 'run', '--rm', '--no-deps', '--no-build', 'migrate']));
  await run(compose(['run', '--rm', '--no-deps', '--no-build', '-T', 'api', 'identity-admin-cli', 'bootstrap', '--email', adminEmail]), adminPassword + '\n');
  await run(compose(['up', '-d', '--no-build', '--wait', '--wait-timeout', '120', 'api', 'worker', 'edge']));
  console.log('Waiting for domain HTTPS certificates and API readiness.');
  await waitHttps(`${issuer}/health/ready`);
  const session = new ApiSession(issuer); const login = await session.request('/api/v1/auth/login/password', { email: adminEmail, password: adminPassword }); if (login.status !== 'authenticated') throw new Error('New administrator login did not authenticate.');
  await session.request('/api/v1/me/reauth/password', { password: adminPassword }); const setup = await session.request('/api/v1/me/mfa/totp/enrollment', {});
  await writeFile(join(local, 'administrator-totp-setup.txt'), setup.secret + '\n', { flag: 'wx', mode: 0o600 });
  console.log(`Add this TOTP key to your authenticator: ${setup.secret}`);
  let recovery; for (let attempts = 0; attempts < 3; attempts++) { const value = await required('Enter the 6-digit authenticator code', (v) => /^\d{6}$/u.test(v)); try { recovery = await session.request('/api/v1/me/mfa/totp/enrollment/confirm', { challenge_id: setup.challenge_id, code: value }); break; } catch { console.log('Authenticator verification failed. Check device time and retry.'); } }
  if (!recovery) throw new Error('TOTP binding failed; administrator initialization is incomplete.');
  await writeFile(join(local, 'administrator-recovery-codes.txt'), recovery.recovery_codes.codes.join('\n') + '\n', { flag: 'wx', mode: 0o600 }); console.log(`Recovery codes saved: ${join(local, 'administrator-recovery-codes.txt')}. Move a copy to a separate trusted location.`);
  for (const [name, domain] of [['a', appA], ['b', appB]]) { const created = await session.request('/api/v1/admin/clients', { name: `CDNGOD demo ${name.toUpperCase()}`, allowed_scopes: ['openid', 'profile', 'email'], redirect_uris: [`https://${domain}/bff/callback`], post_logout_redirect_uris: [`https://${domain}/`] }); const file = join(secrets, `demo-${name}-secret`); await writeFile(file, created.client_secret, { mode: 0o600 }); await chown(file, 10001, 10001); const path = join(local, `demo-${name}.env`); await writeFile(path, (await readFile(path, 'utf8')).replace(`BFF_CLIENT_ID=installer-demo-${name}`, `BFF_CLIENT_ID=${created.client.client_id}`), { mode: 0o600 }); }
  await session.request('/api/v1/auth/logout', {});
  // First installation uses the same verified backup/migration/readiness state recording as upgrades.
  const config = configuration({ ...deployConfig, mode: 'production', release: manifest.revision, composeFile: join(root, 'infra/compose.prod.yaml'), rustImage: manifest.images.runtime, edgeImage: manifest.images.edge, facts: { program: '/internal/facts', args: [] } }, 'production');
  await runRelease(config, 'deploy', { executor: async (program, args, options) => {
    if (program === '/internal/facts') return { code: 0, output: JSON.stringify(await databaseFacts(config)) };
    if (program === 'docker' && args[0] === 'compose' && (args.includes('up') || args.includes('run')) && !args.includes('--no-build')) { const index = args.indexOf(args.includes('up') ? 'up' : 'run'); args = [...args.slice(0, index + 1), '--no-build', ...args.slice(index + 1)]; }
    return executeCommand(program, args, options);
  } });
  await rm(join(local, 'administrator-totp-setup.txt'));
  const backupService = `[Unit]\nDescription=Verified CDNGOD encrypted PostgreSQL backup\nRequiresMountsFor=${backupMount}\nAfter=docker.service\n[Service]\nType=oneshot\nUser=root\nUMask=0077\nExecStart=${process.execPath} ${operations} backup ${root}\nNoNewPrivileges=true\nPrivateTmp=true\nTimeoutStartSec=1h\n`;
  await writeFile('/etc/systemd/system/auth-rust-backup.service', backupService);
  await writeFile('/etc/systemd/system/auth-rust-backup.timer', '[Unit]\nDescription=Daily CDNGOD database backup\n[Timer]\nOnCalendar=*-*-* 02:00:00 UTC\nPersistent=true\nUnit=auth-rust-backup.service\n[Install]\nWantedBy=timers.target\n');
  for (const args of [['daemon-reload'], ['enable', '--now', 'auth-rust-backup.timer']]) if ((await executeCommand('systemctl', args)).code) throw new Error('Backup schedule activation failed.');
  await writeFile(join(local, 'installed.json'), JSON.stringify({ version: 1, revision: manifest.revision, completed: new Date().toISOString() }) + '\n', { mode: 0o600 });
  console.log(`Installed with Docker Caddy. Identity: ${issuer}\nDemo A: https://${appA}\nDemo B: https://${appB}\nAdministrator: ${adminEmail}\nPasswords were not stored or printed. Complete real mail, Passkey and production observation acceptance before final release approval.`);
  if (proxy.nginxStoppedByInstaller) {
    const disable = await question('Caddy now owns80/443. Disable Nginx automatic startup to avoid a reboot conflict? y/N', 'N');
    if (disable.toLowerCase() === 'y' && (await executeCommand('systemctl', ['disable', 'nginx'])).code !== 0) throw new Error('Could not disable Nginx automatic startup.');
    console.log('Existing Nginx configuration was retained. Do not start it while Caddy occupies80/443.');
  }
}
async function waitHttps(url) { for (let attempts = 0; attempts < 120; attempts++) { try { const result = await fetch(url, { signal: AbortSignal.timeout(5000) }); if (result.ok) return; } catch { /* bounded readiness */ } await new Promise((done) => setTimeout(done, 2000)); } throw new Error('HTTPS readiness failed. Check DNS, ports80/443 and certificate issuance; no TLS verification is disabled.'); }
export class ApiSession {
  constructor(origin) { this.origin = origin; this.cookies = new Map(); this.csrf = undefined; }
  async request(path, body) {
    if (body !== undefined && !this.csrf) { const csrf = await this.request('/api/v1/auth/csrf'); this.csrf = csrf.csrf_token; }
    const response = await fetch(this.origin + path, { method: body === undefined ? 'GET' : 'POST', redirect: 'manual', signal: AbortSignal.timeout(10000), headers: { ...(this.cookies.size ? { Cookie: [...this.cookies].map(([key, value]) => `${key}=${value}`).join('; ') } : {}), ...(body === undefined ? {} : { Origin: this.origin, 'Content-Type': 'application/json', 'X-CSRF-Token': this.csrf }) }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
    for (const cookie of response.headers.getSetCookie()) { const first = cookie.split(';')[0]; const index = first.indexOf('='); const name = first.slice(0, index); if (cookie.includes('Max-Age=0')) this.cookies.delete(name); else this.cookies.set(name, first.slice(index + 1)); }
    if (body === undefined && path !== '/api/v1/auth/csrf') { if (!response.ok) throw new Error(`API status ${response.status}`); return response.json(); }
    const value = response.status === 204 ? {} : await response.json(); if (!response.ok) throw new Error(`API status ${response.status}`); if (value.csrf_token) this.csrf = value.csrf_token; else if (value.status === 'mfa_required') this.csrf = undefined; return value;
  }
}
if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.dirname, 'install-production.mjs')) { try { await install(resolve(process.argv[2])); } catch (error) { if (nginxStoppedByInstaller && await portAvailable(80) && await portAvailable(443)) await executeCommand('systemctl', ['start', 'nginx']); console.error(`Installation stopped: ${error.message}`); process.exitCode = 1; } }
