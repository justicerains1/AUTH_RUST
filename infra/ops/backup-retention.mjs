// Conservative base-backup retention. WAL, timeline history and keys are never deleted.
import { constants, createReadStream } from 'node:fs';
import { lstat, mkdir, open, readFile, readdir, realpath, rename, rm } from 'node:fs/promises';
import { createHash, randomUUID } from 'node:crypto';
import { spawn } from 'node:child_process';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { verifiedBackupReceipt } from './collect-metrics.mjs';

const DAY = 86400;
const namePattern = /^\d{8}T\d{6}Z-\d+\.tar\.age$/u;
const shaPattern = /^[a-f0-9]{64}$/u;
const lsnPattern = /^[A-F0-9]{1,8}\/[A-F0-9]{1,8}$/u;
const lsn = (text) => { const [high, low] = text.split('/'); return (BigInt(`0x${high}`) << 32n) + BigInt(`0x${low}`); };
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');
function absolute(path) { if (typeof path !== 'string' || !isAbsolute(path) || resolve(path) !== path) throw new Error('Canonical absolute backup paths required.'); return path; }
function integer(value) { return Number.isSafeInteger(value) && value > 0; }
async function directory(path) { absolute(path); const info = await lstat(path); if (!info.isDirectory() || info.isSymbolicLink() || (info.mode & 0o022) !== 0 || await realpath(path) !== path) throw new Error('Controlled real backup directory required.'); }
async function file(path, maximum = 1024 * 1024) {
  const handle = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
  try { const info = await handle.stat(); if (!info.isFile() || info.nlink !== 1 || (info.mode & 0o077) !== 0 || info.size <= 0 || info.size > maximum || process.getuid && info.uid !== process.getuid()) throw new Error('Owner-controlled ordinary backup file required.'); return { bytes: await handle.readFile(), info }; }
  finally { await handle.close(); }
}
async function objectHash(path) {
  const handle = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    const before = await handle.stat(); if (!before.isFile() || before.nlink !== 1 || (before.mode & 0o077) !== 0 || before.size <= 0) throw new Error('Controlled encrypted object required.');
    const digest = createHash('sha256'); const stream = createReadStream(path, { fd: handle.fd, autoClose: false });
    for await (const part of stream) digest.update(part);
    const after = await handle.stat(); if (before.ino !== after.ino || before.size !== after.size || before.mtimeMs !== after.mtimeMs || before.ctimeMs !== after.ctimeMs) throw new Error('Encrypted object changed while verified.');
    return { sha256: digest.digest('hex'), bytes: before.size };
  } finally { await handle.close(); }
}
async function publish(path, value) {
  const temporary = `${path}.tmp-${randomUUID()}`; const handle = await open(temporary, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL, 0o600);
  try { await handle.writeFile(JSON.stringify(value, null, 2) + '\n'); await handle.sync(); } finally { await handle.close(); }
  try { await rename(temporary, path); const parent = await open(dirname(path), constants.O_RDONLY); try { await parent.sync(); } finally { await parent.close(); } }
  finally { await rm(temporary, { force: true }); }
}
async function command(program, args, env = {}) {
  return new Promise((done, reject) => {
    const child = spawn(program, args, { env: { ...process.env, ...env, LC_ALL: 'C' }, shell: false, stdio: ['ignore', 'pipe', 'pipe'] });
    let output = ''; child.stdout.on('data', (part) => { if (output.length < 4 * 1024 * 1024) output += part; }); child.stderr.on('data', () => {});
    child.once('error', () => reject(new Error('Required verified backup tool is unavailable.')));
    child.once('close', (code) => code === 0 ? done(output) : reject(new Error('Real backup verification tool failed.')));
  });
}
export function catalogMetadata(manifestText, labelText, controlText, receipt, now) {
  // JSON.parse cannot represent PostgreSQL's 64-bit system ID; preserve its decimal spelling.
  const systemMatches = [...manifestText.matchAll(/"System-Identifier"\s*:\s*(\d+)/gu)];
  const systemMatch = systemMatches.length === 1 ? systemMatches[0] : undefined;
  const manifest = JSON.parse(manifestText);
  const start = /^START TIME: (\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}) UTC$/mu.exec(labelText);
  const timeline = /^START TIMELINE: (\d+)$/mu.exec(labelText);
  const segment = /^Bytes per WAL segment:\s+(\d+)$/mu.exec(controlText);
  const ranges = manifest['WAL-Ranges'];
  if (manifest['PostgreSQL-Backup-Manifest-Version'] !== 2 || !systemMatch || !start || !timeline || !segment || !shaPattern.test(manifest['Manifest-Checksum']) || !Array.isArray(ranges) || ranges.length === 0) throw new Error('Verified PostgreSQL manifest metadata is incomplete.');
  const started = Date.parse(`${start[1].replace(' ', 'T')}Z`) / 1000;
  const walRanges = ranges.map((range) => {
    if (!integer(range.Timeline) || !lsnPattern.test(range['Start-LSN']) || !lsnPattern.test(range['End-LSN'])) throw new Error('Verified WAL range metadata is invalid.');
    if (lsn(range['End-LSN']) <= lsn(range['Start-LSN'])) throw new Error('Verified WAL range is not increasing.');
    return { timeline: range.Timeline, start_lsn: range['Start-LSN'], end_lsn: range['End-LSN'] };
  });
  const segmentBytes = Number(segment[1]);
  if (!integer(started) || started > receipt.completed_at || receipt.completed_at > now || BigInt(systemMatch[1]) <= 0n || BigInt(systemMatch[1]) > 18446744073709551615n || walRanges[0].timeline !== Number(timeline[1]) || !integer(segmentBytes) || segmentBytes < 1048576 || segmentBytes > 1073741824 || !Number.isInteger(Math.log2(segmentBytes))) throw new Error('Verified backup time/timeline metadata is invalid.');
  return { version: 1, backup_name: receipt.backup_name, completed_at: receipt.completed_at, started_at: started, ciphertext_sha256: receipt.ciphertext_sha256, ciphertext_bytes: receipt.ciphertext_bytes,
    system_identifier: systemMatch[1], wal_segment_bytes: segmentBytes, wal_ranges: walRanges, manifest_sha256: hash(manifestText), manifest_checksum: manifest['Manifest-Checksum'], verified_at: now, verified_by: 'pg_verifybackup' };
}
export async function createCatalog(config, now = Math.floor(Date.now() / 1000)) {
  const base = absolute(config.backupDirectory); const work = absolute(config.workDirectory); await directory(base); await directory(work);
  const receipt = await verifiedBackupReceipt(base, now);
  const staging = join(work, `.catalog-${randomUUID()}`); await mkdir(staging, { mode: 0o700 });
  try {
    const restored = join(staging, 'base');
    // restore-base performs checksum selection, age AEAD verification and real pg_verifybackup.
    await command('/bin/sh', [absolute(config.restoreScript), join(base, receipt.backup_name), restored], { AGE_IDENTITY_FILE: absolute(config.ageIdentityFile), AGE_BINARY: absolute(config.ageBinary) });
    const manifestText = (await readFile(join(restored, 'backup_manifest'))).toString();
    const labelText = (await readFile(join(restored, 'backup_label'))).toString();
    const control = await command(absolute(config.pgControlData), [restored]);
    const metadata = catalogMetadata(manifestText, labelText, control, receipt, Math.max(now, Math.floor(Date.now() / 1000)));
    const object = await objectHash(join(base, receipt.backup_name));
    if (object.sha256 !== receipt.ciphertext_sha256 || object.bytes !== receipt.ciphertext_bytes) throw new Error('Selected backup changed after verification.');
    const path = join(base, `${receipt.backup_name}.catalog.json`);
    // Existing catalog is immutable for its object; changing timestamp does not create newer data.
    try { const previous = JSON.parse((await file(path)).bytes); if (previous.ciphertext_sha256 !== metadata.ciphertext_sha256 || previous.manifest_sha256 !== metadata.manifest_sha256 || previous.completed_at !== metadata.completed_at) throw new Error('Conflicting existing backup catalog.'); return previous; }
    catch (error) { if (error.code !== 'ENOENT') throw error; }
    await publish(path, metadata); return metadata;
  } finally { await rm(staging, { recursive: true, force: true }); }
}
function validateCatalog(value) {
  if (value.version !== 1 || !namePattern.test(value.backup_name) || !integer(value.started_at) || !integer(value.completed_at) || value.started_at > value.completed_at || !integer(value.verified_at) || value.verified_at < value.completed_at || !shaPattern.test(value.ciphertext_sha256) || !shaPattern.test(value.manifest_sha256) || !shaPattern.test(value.manifest_checksum) || !integer(value.ciphertext_bytes) || !/^\d{1,20}$/u.test(value.system_identifier) || BigInt(value.system_identifier) <= 0n || BigInt(value.system_identifier) > 18446744073709551615n || value.verified_by !== 'pg_verifybackup' || !integer(value.wal_segment_bytes) || !Array.isArray(value.wal_ranges) || value.wal_ranges.length === 0 || value.wal_ranges.some((range) => !integer(range.timeline) || !lsnPattern.test(range.start_lsn) || !lsnPattern.test(range.end_lsn) || lsn(range.end_lsn) <= lsn(range.start_lsn))) throw new Error('Malformed backup catalog.');
}
export function retentionPlan(catalogs, proofs, { now, keepDays = 14, pinned = [] }) {
  if (!integer(now) || !Number.isSafeInteger(keepDays) || keepDays < 14) throw new Error('Retention must preserve at least fourteen days.');
  const cutoff = now - keepDays * DAY;
  catalogs.forEach(validateCatalog);
  const keep = catalogs.map((entry) => entry.backup_name);
  const held = (reason) => ({ version: 1, cutoff, keep_days: keepDays, hold: reason, keep, candidates: [], wal_policy: 'keep-all-wal-history-backup-labels-and-keys' });
  if (!catalogs.length) return held('no-verified-backups');
  if (new Set(catalogs.map((entry) => entry.system_identifier)).size !== 1) return held('multiple-clusters-require-separate-reviewed-policy');
  const timelines = new Set(catalogs.flatMap((entry) => entry.wal_ranges.map((range) => range.timeline)));
  if (new Set(catalogs.map((entry) => entry.wal_segment_bytes)).size !== 1) return held('inconsistent-wal-segment-sizes-held');
  if (timelines.size !== 1 || catalogs.some((entry) => entry.wal_ranges.length !== 1)) return held('timeline-forks-or-unknown-ranges-held');
  if (catalogs.some((entry) => entry.completed_at > now + 60 || entry.verified_at > now + 60)) return held('future-metadata-held');
  const eligible = catalogs.filter((entry) => entry.completed_at <= cutoff).sort((a, b) => b.completed_at - a.completed_at);
  const anchor = eligible[0]; if (!anchor) return held('no-complete-anchor-before-window');
  const proof = proofs[anchor.backup_name];
  if (!proof || proof.version !== 1 || proof.restore_verified !== true || proof.pg_verifybackup_passed !== true || proof.backup_name !== anchor.backup_name || proof.ciphertext_sha256 !== anchor.ciphertext_sha256 || proof.manifest_sha256 !== anchor.manifest_sha256 || proof.system_identifier !== anchor.system_identifier || proof.recovered_timeline !== anchor.wal_ranges[0].timeline || !lsnPattern.test(proof.replayed_lsn ?? '') || lsn(proof.replayed_lsn) < lsn(anchor.wal_ranges[0].end_lsn) || !integer(proof.verification_completed_at) || proof.verification_completed_at < anchor.completed_at || proof.verification_completed_at > now + 60 || !integer(proof.recovery_target_time) || proof.recovery_target_time < cutoff || proof.recovery_target_time > proof.verification_completed_at || !Array.isArray(proof.checks) || !proof.checks.includes('postgres-recovery') || !proof.checks.includes('business-validation')) return held('anchor-lacks-object-bound-real-restore-proof-through-window');
  const retained = catalogs.filter((entry) => entry.completed_at >= anchor.completed_at || pinned.includes(entry.backup_name)).map((entry) => entry.backup_name);
  return { version: 1, cutoff, keep_days: keepDays, anchor: anchor.backup_name, keep: retained, candidates: catalogs.filter((entry) => !retained.includes(entry.backup_name)).map((entry) => entry.backup_name), wal_policy: 'keep-all-wal-history-backup-labels-and-keys' };
}
export async function planRetention(config, now = Math.floor(Date.now() / 1000), ignoredQuarantine) {
  const base = absolute(config.backupDirectory); await directory(base); const names = await readdir(base);
  if (names.some((name) => name.startsWith('.base-backup.') || name.startsWith('.base-restore.') || name.startsWith('.catalog-') || name === '.restore-active' || name.startsWith('.retention-quarantine-') && name !== ignoredQuarantine)) return { version: 1, hold: 'active-or-interrupted-backup-verify-held', candidates: [] };
  const objects = names.filter((name) => namePattern.test(name)); const catalogs = []; const proofs = {}; const pinned = [];
  for (const name of objects) {
    try {
      const catalog = JSON.parse((await file(join(base, `${name}.catalog.json`))).bytes); validateCatalog(catalog);
      if (catalog.backup_name !== name) throw new Error('Catalog name mismatch.');
      const object = await objectHash(join(base, name)); if (object.sha256 !== catalog.ciphertext_sha256 || object.bytes !== catalog.ciphertext_bytes || (await file(join(base, `${name}.sha256`), 2048)).bytes.toString() !== `${object.sha256}  ${name}\n`) throw new Error('Catalog object does not match.');
      catalogs.push(catalog);
      try { proofs[name] = JSON.parse((await file(join(base, `${name}.restore-proof.json`))).bytes); } catch (error) { if (error.code !== 'ENOENT') throw error; }
      if (names.includes(`${name}.pin`)) { await file(join(base, `${name}.pin`), 4096); pinned.push(name); }
    } catch { return { version: 1, hold: 'missing-corrupt-or-unverified-object-held', candidates: [], keep: objects }; }
  }
  return retentionPlan(catalogs, proofs, { now, keepDays: config.keepDays ?? 14, pinned });
}
export async function applyRetention(config, { now = Math.floor(Date.now() / 1000), reviewedPlanSha256 } = {}) {
  const base = absolute(config.backupDirectory); await directory(base);
  if (config.exclusiveMaintenanceAcknowledged !== true) throw new Error('Apply requires an explicitly quiesced backup/restore maintenance window; restore users must pin needed objects.');
  const lock = join(base, '.retention-lock'); await mkdir(lock, { mode: 0o700 });
  try {
    const plan = await planRetention(config, now); if (plan.hold) throw new Error('Retention is held; no backup may be removed.');
    if (!shaPattern.test(reviewedPlanSha256 ?? '') || hash(JSON.stringify(plan)) !== reviewedPlanSha256) throw new Error('An exact reviewed current retention plan is required.');
    // Atomically quarantine selected base objects only. No wildcard, WAL, keys or active PGDATA.
    const quarantineName = `.retention-quarantine-${randomUUID()}`;
    const quarantine = join(base, quarantineName); await mkdir(quarantine, { mode: 0o700 });
    for (const name of plan.candidates) {
      if (!namePattern.test(name)) throw new Error('Invalid deletion candidate.');
      const current = await planRetention(config, now, quarantineName);
      if (current.hold || current.anchor !== plan.anchor || !current.candidates.includes(name)) throw new Error('Recovery chain or restore pin changed; retention stopped.');
      for (const suffix of ['', '.sha256', '.catalog.json', '.restore-proof.json']) {
        const path = join(base, name + suffix); try { await rename(path, join(quarantine, name + suffix)); } catch (error) { if (suffix !== '.restore-proof.json' || error.code !== 'ENOENT') throw error; }
      }
    }
    const parent = await open(base, constants.O_RDONLY); try { await parent.sync(); } finally { await parent.close(); }
    await rm(quarantine, { recursive: true }); return { ...plan, deleted: plan.candidates };
  } finally { await rm(lock, { recursive: true, force: true }); }
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const [action, configPath, confirmation] = process.argv.slice(2); const config = JSON.parse((await file(absolute(configPath), 16384)).bytes);
    if (action === 'catalog') { const value = await createCatalog(config); console.log(JSON.stringify({ cataloged: value.backup_name, ciphertext_sha256: value.ciphertext_sha256 })); }
    else if (action === 'plan') { const plan = await planRetention(config); console.log(JSON.stringify({ ...plan, plan_sha256: hash(JSON.stringify(plan)) }, null, 2)); }
    else if (action === 'apply') console.log(JSON.stringify(await applyRetention(config, { reviewedPlanSha256: confirmation })));
    else throw new Error('Use catalog, plan or apply with a controlled config.');
  } catch { console.error('Backup retention could not complete; inspect the controlled plan and any quarantine before retrying. WAL, timeline history and keys are never deleted.'); process.exitCode = 1; }
}
