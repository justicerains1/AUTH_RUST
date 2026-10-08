// Synthetic catalog/file tests verify retention logic only, never claim a real restore.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, readFile, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { applyRetention, catalogMetadata, planRetention, retentionPlan } from '../../infra/ops/backup-retention.mjs';

const now = 2_000_000; const cutoff = now - 14 * 86400;
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');
function catalog(id, completed = cutoff, extra = {}) {
  return { version: 1, backup_name: `20261008T000000Z-${id}.tar.age`, completed_at: completed, started_at: completed - 100, verified_at: completed + 10, ciphertext_sha256: 'a'.repeat(64), ciphertext_bytes: 10, system_identifier: '18446744073709551614', wal_segment_bytes: 16777216, wal_ranges: [{ timeline: 1, start_lsn: '0/1000000', end_lsn: '0/2000000' }], manifest_sha256: 'b'.repeat(64), manifest_checksum: 'c'.repeat(64), verified_by: 'pg_verifybackup', ...extra };
}
function proof(anchor, extra = {}) { return { version: 1, backup_name: anchor.backup_name, ciphertext_sha256: anchor.ciphertext_sha256, manifest_sha256: anchor.manifest_sha256, system_identifier: anchor.system_identifier, restore_verified: true, pg_verifybackup_passed: true, recovered_timeline: 1, replayed_lsn: anchor.wal_ranges[0].end_lsn, verification_completed_at: now, recovery_target_time: cutoff, checks: ['postgres-recovery', 'business-validation'], ...extra }; }
test('window anchor must finish before cutoff; all newer backups and restore pins remain', () => {
  const old = catalog(1, cutoff - 1000); const anchor = catalog(2); const ongoingAtCutoff = catalog(3, cutoff + 1, { started_at: cutoff - 200 }); const latest = catalog(4, now - 30);
  const plan = retentionPlan([old, anchor, ongoingAtCutoff, latest], { [anchor.backup_name]: proof(anchor) }, { now });
  assert.equal(plan.anchor, anchor.backup_name); assert.deepEqual(plan.candidates, [old.backup_name]); assert.ok(plan.keep.includes(ongoingAtCutoff.backup_name)); assert.equal(plan.wal_policy, 'keep-all-wal-history-backup-labels-and-keys');
  assert.deepEqual(retentionPlan([old, anchor], { [anchor.backup_name]: proof(anchor) }, { now, pinned: [old.backup_name] }).candidates, []);
  assert.throws(() => retentionPlan([anchor], {}, { now, keepDays: 13 }), /fourteen/u);
});
test('no anchor, missing real-proof binding, target before window, mixed cluster or fork holds everything', () => {
  const anchor = catalog(1); const old = catalog(2, cutoff - 100);
  const cases = [
    [[catalog(3, cutoff + 1)], {}], [[old, anchor], {}],
    [[old, anchor], { [anchor.backup_name]: proof(anchor, { ciphertext_sha256: 'd'.repeat(64) }) }],
    [[old, anchor], { [anchor.backup_name]: proof(anchor, { recovery_target_time: cutoff - 1 }) }],
    [[old, anchor], { [anchor.backup_name]: proof(anchor, { recovered_timeline: 2 }) }],
    [[old, anchor], { [anchor.backup_name]: proof(anchor, { replayed_lsn: '0/1' }) }],
    [[old, catalog(3, cutoff, { system_identifier: '5' })], {}],
    [[old, catalog(3, cutoff, { wal_ranges: [{ timeline: 2, start_lsn: '0/2000000', end_lsn: '0/3000000' }] })], {}],
  ];
  for (const [catalogs, proofs] of cases) { const plan = retentionPlan(catalogs, proofs, { now }); assert.ok(plan.hold); assert.deepEqual(plan.candidates, []); }
});
test('verified manifest parser preserves uint64 system ID and PG start/end LSN', () => {
  const text = '{"PostgreSQL-Backup-Manifest-Version":2,"System-Identifier":18446744073709551614,"WAL-Ranges":[{"Timeline":1,"Start-LSN":"0/1000000","End-LSN":"0/2000000"}],"Manifest-Checksum":"' + 'c'.repeat(64) + '"}';
  const completed = Date.parse('2026-10-08T00:01:00Z') / 1000;
  const metadata = catalogMetadata(text, 'START TIME: 2026-10-08 00:00:00 UTC\nSTART TIMELINE: 1\n', 'Bytes per WAL segment: 16777216\n', { backup_name: '20261008T000000Z-1.tar.age', completed_at: completed, ciphertext_sha256: 'a'.repeat(64), ciphertext_bytes: 10 }, completed + 1);
  assert.equal(metadata.system_identifier, '18446744073709551614'); assert.equal(metadata.wal_ranges[0].end_lsn, '0/2000000');
  assert.throws(() => catalogMetadata(text.replace('0/2000000', '0/1'), 'START TIME: 2026-10-08 00:00:00 UTC\nSTART TIMELINE: 1\n', 'Bytes per WAL segment: 16777216\n', metadata, completed + 1), /increasing/u);
});
async function fixture(t) {
  const base = await mkdtemp(join(tmpdir(), 'retention-fixture-')); t.after(() => rm(base, { recursive: true, force: true }));
  const old = catalog(1, cutoff - 1000); const anchor = catalog(2); const latest = catalog(3, now - 30);
  for (const entry of [old, anchor, latest]) {
    const data = Buffer.from(`Synthetic controlled fixture ${entry.backup_name}`); entry.ciphertext_sha256 = sha(data); entry.ciphertext_bytes = data.length;
    await writeFile(join(base, entry.backup_name), data, { mode: 0o600 }); await writeFile(join(base, `${entry.backup_name}.sha256`), `${entry.ciphertext_sha256}  ${entry.backup_name}\n`, { mode: 0o600 }); await writeFile(join(base, `${entry.backup_name}.catalog.json`), JSON.stringify(entry), { mode: 0o600 });
  }
  await writeFile(join(base, `${anchor.backup_name}.restore-proof.json`), JSON.stringify(proof(anchor)), { mode: 0o600 });
  await mkdir(join(base, 'wal')); await writeFile(join(base, 'wal/000000010000000000000001'), 'always retained');
  return { config: { backupDirectory: base, keepDays: 14, exclusiveMaintenanceAcknowledged: true }, base, old, anchor, latest };
}
test('explicit matching reviewed plan removes only older base objects; WAL/anchor/latest untouched', async (t) => {
  const { config, base, old, anchor, latest } = await fixture(t); const plan = await planRetention(config, now);
  await assert.rejects(applyRetention({ ...config, exclusiveMaintenanceAcknowledged: false }, { now, reviewedPlanSha256: sha(JSON.stringify(plan)) }), /quiesced/u);
  await assert.rejects(applyRetention(config, { now, reviewedPlanSha256: '0'.repeat(64) }), /reviewed/u);
  const result = await applyRetention(config, { now, reviewedPlanSha256: sha(JSON.stringify(plan)) }); assert.deepEqual(result.deleted, [old.backup_name]);
  await assert.rejects(readFile(join(base, old.backup_name)), { code: 'ENOENT' }); assert.ok(await readFile(join(base, anchor.backup_name))); assert.ok(await readFile(join(base, latest.backup_name))); assert.equal((await readFile(join(base, 'wal/000000010000000000000001'))).toString(), 'always retained');
});
test('missing/corrupt/symlink objects and active restore marker prevent all apply', async (t) => {
  const { config, base, old } = await fixture(t); await writeFile(join(base, '.restore-active'), 'controlled restore in progress', { mode: 0o600 }); assert.ok((await planRetention(config, now)).hold); await rm(join(base, '.restore-active'));
  await rm(join(base, old.backup_name)); await symlink('/does/not/exist', join(base, old.backup_name)); assert.ok((await planRetention(config, now)).hold); await assert.rejects(applyRetention(config, { now, reviewedPlanSha256: 'a'.repeat(64) }), /held/u);
});
test('a restore pin added after dry-run invalidates its reviewed plan and keeps old data', async (t) => {
  const { config, base, old } = await fixture(t); const plan = await planRetention(config, now); await writeFile(join(base, `${old.backup_name}.pin`), 'active restore pin', { mode: 0o600 }); await assert.rejects(applyRetention(config, { now, reviewedPlanSha256: sha(JSON.stringify(plan)) }), /reviewed/u); assert.ok(await readFile(join(base, old.backup_name)));
});
