// Owner-controlled, scheduled host checks for a local Prometheus textfile collector.
// No credentials, paths, hostnames, account IDs or backup IDs appear in metric labels or diagnostics.
import { constants, createReadStream } from 'node:fs';
import { lstat, open, readFile, rename, rm, stat, statfs } from 'node:fs/promises';
import { createHash, randomUUID } from 'node:crypto';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { connect } from 'node:tls';
import { isIP } from 'node:net';

const checks = ['disk_data', 'disk_backup', 'issuer_tls', 'base_backup'];
function absolute(value) { if (typeof value !== 'string' || !isAbsolute(value) || resolve(value) !== value) throw new Error('Explicit canonical absolute paths required.'); return value; }
function device(value) { if (typeof value !== 'string' || !/^\d+$/u.test(value)) throw new Error('Explicit expected filesystem device numbers required.'); return BigInt(value); }
export function configuration(env) {
  const issuer = new URL(env.ISSUER); if (issuer.protocol !== 'https:' || issuer.username || issuer.password || issuer.search || issuer.hash || issuer.pathname !== '/') throw new Error('A fixed HTTPS issuer origin is required.');
  return { dataDirectory: absolute(env.OPS_DATA_DIRECTORY), dataDevice: device(env.OPS_DATA_DEVICE), backupDirectory: absolute(env.OPS_BACKUP_DIRECTORY), backupDevice: device(env.OPS_BACKUP_DEVICE), issuer, caFile: env.OPS_TLS_CA_FILE ? absolute(env.OPS_TLS_CA_FILE) : undefined, output: absolute(env.OPS_METRICS_FILE) };
}
async function disk(directory, expectedDevice) {
  const info = await stat(directory, { bigint: true }); if (!info.isDirectory() || info.dev !== expectedDevice) throw new Error('Configured filesystem is unavailable.');
  const fs = await statfs(directory, { bigint: true }); const size = fs.blocks * fs.bsize; const available = fs.bavail * fs.bsize;
  if (size <= 0n || available < 0n || available > size || size > BigInt(Number.MAX_SAFE_INTEGER)) throw new Error('Invalid filesystem measurement.');
  return { size: Number(size), available: Number(available) };
}
async function controlledFile(path, maximum) {
  const handle = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
  try { const info = await handle.stat(); if (!info.isFile() || info.nlink !== 1 || (info.mode & 0o077) !== 0 || info.size <= 0 || info.size > maximum) throw new Error('Controlled backup file required.'); return { handle, info }; }
  catch (error) { await handle.close(); throw error; }
}
async function smallFile(path, maximum) { const { handle } = await controlledFile(path, maximum); try { return await handle.readFile('utf8'); } finally { await handle.close(); } }
export async function verifiedBackupReceipt(directory, now) {
  const parent = await stat(directory); if (!parent.isDirectory() || (parent.mode & 0o022) !== 0) throw new Error('Owner-controlled backup directory required.');
  const receipt = JSON.parse(await smallFile(join(directory, 'last-success.json'), 4096));
  if (JSON.stringify(Object.keys(receipt).sort()) !== JSON.stringify(['backup_name', 'ciphertext_bytes', 'ciphertext_sha256', 'completed_at', 'version']) || receipt.version !== 1 || !Number.isSafeInteger(receipt.completed_at) || receipt.completed_at <= 0 || receipt.completed_at > now + 60 || !Number.isSafeInteger(receipt.ciphertext_bytes) || receipt.ciphertext_bytes <= 0 || !/^[a-f0-9]{64}$/u.test(receipt.ciphertext_sha256) || typeof receipt.backup_name !== 'string' || !/^\d{8}T\d{6}Z-\d+\.tar\.age$/u.test(receipt.backup_name)) throw new Error('Invalid backup completion record.');
  const object = join(directory, receipt.backup_name);
  if (await smallFile(`${object}.sha256`, 2048) !== `${receipt.ciphertext_sha256}  ${receipt.backup_name}\n`) throw new Error('Backup manifest does not match completion.');
  const { handle, info } = await controlledFile(object, Number.MAX_SAFE_INTEGER);
  try {
    if (info.size !== receipt.ciphertext_bytes) throw new Error('Backup size does not match completion.');
    const digest = createHash('sha256'); const stream = createReadStream(object, { fd: handle.fd, autoClose: false }); const timer = setTimeout(() => { stream.destroy(new Error('Backup checksum deadline exceeded.')); }, 120_000);
    try { for await (const chunk of stream) digest.update(chunk); } finally { clearTimeout(timer); }
    const after = await handle.stat();
    if (after.size !== info.size || after.ino !== info.ino || after.dev !== info.dev || after.mtimeMs !== info.mtimeMs || after.ctimeMs !== info.ctimeMs || digest.digest('hex') !== receipt.ciphertext_sha256) throw new Error('Selected backup changed or failed checksum.');
    return Object.freeze(receipt);
  } finally { await handle.close(); }
}
export async function backupCompletion(directory, now) { return (await verifiedBackupReceipt(directory, now)).completed_at; }
export async function certificateExpiry(issuer, caFile) {
  const ca = caFile ? await readFile(caFile) : undefined;
  const host = issuer.hostname.replace(/^\[|\]$/gu, '');
  return new Promise((done, fail) => {
    const socket = connect({ host, port: Number(issuer.port || 443), ...(isIP(host) ? {} : { servername: host }), rejectUnauthorized: true, ...(ca ? { ca } : {}) });
    const timer = setTimeout(() => { socket.destroy(new Error('TLS deadline exceeded.')); }, 5000);
    socket.once('error', (error) => { clearTimeout(timer); fail(error); });
    socket.once('secureConnect', () => { clearTimeout(timer); const expires = Date.parse(socket.getPeerCertificate().valid_to) / 1000; const authorized = socket.authorized; socket.end(); if (!authorized || !Number.isFinite(expires) || expires <= 0) fail(new Error('Verified issuer certificate required.')); else done(expires); });
  });
}
export async function collectMetrics(config) {
  const now = Math.floor(Date.now() / 1000); const values = { disk_data: { size: 0, available: 0 }, disk_backup: { size: 0, available: 0 }, issuer_tls: 0, base_backup: 0 }; const success = Object.fromEntries(checks.map((check) => [check, 0]));
  const tasks = [() => disk(config.dataDirectory, config.dataDevice), () => disk(config.backupDirectory, config.backupDevice), () => certificateExpiry(config.issuer, config.caFile), async () => { await disk(config.backupDirectory, config.backupDevice); return backupCompletion(config.backupDirectory, now); }];
  const results = await Promise.allSettled(tasks.map((task) => task()));
  results.forEach((result, index) => { if (result.status === 'fulfilled') { values[checks[index]] = result.value; success[checks[index]] = 1; } });
  const metric = (name, description, samples) => `# HELP ${name} ${description}\n# TYPE ${name} gauge\n${samples.map(([labels, value]) => `${name}${labels} ${value}\n`).join('')}`;
  const text = metric('identity_ops_collection_success', 'Whether each fixed host check succeeded.', checks.map((check) => [`{check="${check}"}`, success[check]]))
    + metric('identity_ops_collection_timestamp_seconds', 'Unix time of the last attempt, including failed checks.', checks.map((check) => [`{check="${check}"}`, now]))
    + metric('identity_ops_disk_size_bytes', 'Size of the explicitly expected filesystem.', [['{volume="data"}', values.disk_data.size], ['{volume="backup"}', values.disk_backup.size]])
    + metric('identity_ops_disk_available_bytes', 'Filesystem bytes available to the collection user.', [['{volume="data"}', values.disk_data.available], ['{volume="backup"}', values.disk_backup.available]])
    + metric('identity_ops_certificate_not_after_timestamp_seconds', 'Expiration of the CA and hostname verified issuer leaf certificate.', [['', values.issuer_tls]])
    + metric('identity_ops_base_backup_completed_timestamp_seconds', 'Last verified encrypted base backup completion, never file mtime.', [['', values.base_backup]]);
  return { text, success: checks.every((check) => success[check] === 1), checks: success };
}
export async function publishMetrics(output, text) {
  absolute(output); if (!output.endsWith('.prom')) throw new Error('A local Prometheus textfile destination is required.');
  const parent = dirname(output); const info = await stat(parent); if (!info.isDirectory() || (info.mode & 0o022) !== 0 || (process.getuid && info.uid !== process.getuid())) throw new Error('An owner-controlled output directory is required.');
  try { const existing = await lstat(output); if (!existing.isFile() || (existing.mode & 0o022) !== 0) throw new Error('Unsafe metrics destination.'); } catch (error) { if (error.code !== 'ENOENT') throw error; }
  const temporary = `${output}.tmp-${randomUUID()}`; let handle;
  try { handle = await open(temporary, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL, 0o640); await handle.writeFile(text); await handle.sync(); await handle.close(); handle = undefined; await rename(temporary, output); const directory = await open(parent, constants.O_RDONLY); try { await directory.sync(); } finally { await directory.close(); } }
  finally { if (handle) await handle.close(); await rm(temporary, { force: true }); }
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { const config = configuration(process.env); const result = await collectMetrics(config); await publishMetrics(config.output, result.text); console.log(`Identity host checks published; ${result.success ? 'all checks succeeded' : 'one or more checks failed'}.`); process.exitCode = result.success ? 0 : 1; }
  catch { console.error('Identity host checks could not publish; inspect controlled configuration and monitoring freshness.'); process.exitCode = 1; }
}
