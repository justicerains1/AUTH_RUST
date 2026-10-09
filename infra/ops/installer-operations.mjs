// Runtime helpers shipped in the CI bundle, not build tools.
import { readFile, stat, open, rm, rename } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { randomUUID } from 'node:crypto';
import { executeCommand } from './release.mjs';
import { verifiedBackupReceipt } from './collect-metrics.mjs';

export async function backup(root, executor = executeCommand) {
  const settings = JSON.parse(await readFile(join(root, '.local/production/installer.json'), 'utf8'));
  if (settings.backupMount && (await executor('mountpoint', ['-q', settings.backupMount])).code !== 0) throw new Error('Independent backup mount is unavailable.');
  const actual = String((await stat(join(settings.backupRoot, 'wal'), { bigint: true })).dev); if (actual !== settings.backupDevice) throw new Error('Backup storage device changed or is unavailable.');
  const compose = ['compose', '--project-name', settings.project, '--env-file', join(root, 'infra/production.env'), '-f', join(root, 'infra/compose.prod.yaml')];
  const id = await executor('docker', [...compose, 'ps', '-q', 'postgres']); if (id.code || !/^[a-f0-9]{12,64}$/u.test(id.output.trim())) throw new Error('Actual PostgreSQL container unavailable.');
  const container = id.output.trim();
  // pg_basebackup and pg_verifybackup run in the existing PG image; no packages compiled/installed.
  const script = `set -eu\numask 077\ntemp=$(mktemp -d)\ntrap 'rm -rf "$temp"' EXIT\npg_basebackup -U identity -h /var/run/postgresql --pgdata "$temp/base" --format=plain --wal-method=stream --checkpoint=fast --no-password\npg_verifybackup "$temp/base" >&2\ntar -C "$temp" -cf "$temp/base.tar" base\n/opt/identity-ops/age --encrypt --recipient "$AGE_RECIPIENT" "$temp/base.tar"\n`;
  const name = `${new Date().toISOString().replaceAll('-', '').replaceAll(':', '').slice(0, 15)}Z-${process.pid}.tar.age`; const directory = join(settings.backupRoot, 'base'); const temporary = join(directory, `.install-backup-${randomUUID()}`); const target = join(directory, name);
  const output = await open(temporary, 'wx', 0o600);
  try {
    const { spawn } = await import('node:child_process');
    const result = await new Promise((done) => { const child = spawn('docker', ['exec', '-i', container, 'sh', '-s'], { stdio: ['pipe', output.fd, 'pipe'], shell: false }); let error = ''; child.stderr.on('data', (part) => { if (error.length < 4096) error += part; }); child.stdin.end(script); child.on('error', () => done(127)); child.on('close', (code) => done(code)); });
    if (result !== 0) throw new Error('Actual database backup/verify/encryption failed.'); await output.sync(); await output.close(); await rename(temporary, target);
    const { createReadStream } = await import('node:fs'); const { createHash } = await import('node:crypto'); const hash = createHash('sha256'); for await (const chunk of createReadStream(target)) hash.update(chunk); const digest = hash.digest('hex');
    const receipt = { version: 1, completed_at: Math.floor(Date.now() / 1000), backup_name: name, ciphertext_bytes: (await stat(target)).size, ciphertext_sha256: digest };
    const manifest = await open(`${target}.sha256`, 'wx', 0o600); await manifest.writeFile(`${digest}  ${name}\n`); await manifest.sync(); await manifest.close();
    const parent = await open(directory, 'r'); await parent.sync();
    const next = join(directory, `.receipt-${randomUUID()}`); const handle = await open(next, 'wx', 0o600); await handle.writeFile(JSON.stringify(receipt) + '\n'); await handle.sync(); await handle.close(); await rename(next, join(directory, 'last-success.json')); await parent.sync(); await parent.close();
    await verifiedBackupReceipt(directory, Math.floor(Date.now() / 1000));
  } finally { await output.close().catch(() => {}); await rm(temporary, { force: true }); }
}
export async function smoke(root) {
  const settings = JSON.parse(await readFile(join(root, '.local/production/installer.json'), 'utf8'));
  for (const host of [settings.identity, settings.appA, settings.appB]) { const response = await fetch(`https://${host}/`, { signal: AbortSignal.timeout(10000) }); if (!response.ok || !response.headers.get('content-security-policy') || !response.headers.get('strict-transport-security')) throw new Error('HTTPS page/security headers failed.'); }
  const ready = await fetch(`${settings.issuer}/health/ready`, { signal: AbortSignal.timeout(10000) }); if (!ready.ok || (await ready.json()).status !== 'ready') throw new Error('Identity readiness failed.');
  const discovery = await fetch(`${settings.issuer}/.well-known/openid-configuration`, { signal: AbortSignal.timeout(10000) }); if (!discovery.ok || (await discovery.json()).issuer !== settings.issuer) throw new Error('OIDC issuer mismatch.');
  const csrf = await fetch(`${settings.issuer}/api/v1/auth/csrf`, { signal: AbortSignal.timeout(10000) }); if (!csrf.ok || !csrf.headers.getSetCookie().some((cookie) => cookie.includes('__Host-identity-preauth=') && cookie.includes('Secure') && cookie.includes('HttpOnly'))) throw new Error('Production cookie boundary failed.');
  console.log('Service readiness smoke passed; real SMTP delivery, physical Passkey and full browser business acceptance remain separate release gates.');
}
if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.dirname, 'installer-operations.mjs')) { try { const root = resolve(process.argv[3]); if (process.argv[2] === 'backup') await backup(root); else if (process.argv[2] === 'smoke') await smoke(root); else throw new Error('Use backup or smoke.'); } catch (error) { console.error(error.message); process.exitCode = 1; } }
