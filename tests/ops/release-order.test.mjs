// Pure process-order tests only; real container/identity tests are a separate local drill.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { configuration, runRelease } from '../../infra/ops/release.mjs';
import { verifiedBackupReceipt } from '../../infra/ops/collect-metrics.mjs';

async function fixture(t) {
  const directory = await mkdtemp(join(tmpdir(), 'identity-release-order-')); t.after(() => rm(directory, { recursive: true, force: true }));
  const backupDirectory = join(directory, 'backup'); await mkdir(backupDirectory, { mode: 0o700 });
  return configuration({ mode: 'local-review', project: 'identity-order-test', release: 'new', rustImage: 'auth-rust-runtime:new', edgeImage: 'auth-rust-edge:new', composeFile: join(directory, 'compose.yaml'), envFile: join(directory, 'release.env'), stateDirectory: join(directory, 'state'), backupDirectory, backup: { program: '/controlled/backup', args: [] }, smoke: { program: '/controlled/smoke', args: [] }, facts: { program: '/controlled/facts', args: [] }, services: ['api', 'edge'], configChecks: [['api', 'identity-server']] }, 'local-review');
}
const facts = { version: 1, migration_sha256: 'a'.repeat(64), used_kids: ['active'], configured_kids: ['active'] };
const output = (program, config) => program === config.facts.program ? JSON.stringify(facts) : 'controlled child';
async function backup(config, now, id = 1) {
  const backup_name = `20261008T000000Z-${id}.tar.age`; const contents = Buffer.from('controlled synthetic encrypted-object fixture; not a PostgreSQL backup'); const ciphertext_sha256 = createHash('sha256').update(contents).digest('hex');
  await writeFile(join(config.backupDirectory, backup_name), contents, { mode: 0o600 });
  await writeFile(join(config.backupDirectory, `${backup_name}.sha256`), `${ciphertext_sha256}  ${backup_name}\n`, { mode: 0o600 });
  await writeFile(join(config.backupDirectory, 'last-success.json'), JSON.stringify({ version: 1, completed_at: now, backup_name, ciphertext_bytes: contents.length, ciphertext_sha256 }), { mode: 0o600 });
}
test('release ordering verifies new backup before migrate, up, ready and smoke', async (t) => {
  const config = await fixture(t); const visited = [];
  const record = await runRelease(config, 'deploy', { clock: () => 100, executor: async (program, args) => { visited.push(program === 'docker' ? args.includes('--check-config') ? 'check' : args.includes('migrate') ? 'migrate' : args.includes('--wait') ? 'ready' : args.includes('up') ? 'up' : 'config-or-image' : program); if (program === config.backup.program) await backup(config, 100); return { code: 0, output: output(program, config) }; } });
  assert.ok(visited.indexOf('check') < visited.indexOf(config.backup.program)); assert.ok(visited.indexOf(config.backup.program) < visited.indexOf('migrate')); assert.ok(visited.indexOf('migrate') < visited.indexOf('up')); assert.ok(visited.indexOf('up') < visited.indexOf('ready')); assert.ok(visited.indexOf('ready') < visited.indexOf(config.smoke.program));
  assert.equal(record.status, 'complete'); assert.equal(JSON.parse(await readFile(join(config.stateDirectory, 'current.json'))).release, 'new');
});
test('migration failure never starts new services and retains prior successful state', async (t) => {
  const config = await fixture(t); await mkdir(config.stateDirectory, { mode: 0o700 }); const original = { release: 'old', rustImage: 'auth-rust-runtime:old', edgeImage: 'auth-rust-edge:old' }; await writeFile(join(config.stateDirectory, 'current.json'), JSON.stringify(original), { mode: 0o600 }); const visited = [];
  await assert.rejects(runRelease(config, 'deploy', { clock: () => 100, executor: async (program, args) => { visited.push(args); if (program === config.backup.program) await backup(config, 100); return { code: args.includes('migrate') ? 17 : 0, output: output(program, config) }; } }), /migrate/u);
  assert.equal(visited.some((args) => args.includes('up')), false); assert.deepEqual(JSON.parse(await readFile(join(config.stateDirectory, 'current.json'))), original);
});
test('zero-exit backup command cannot reuse old completion or corrupt ciphertext', async (t) => {
  const config = await fixture(t); await backup(config, 99); let migrated = false;
  await assert.rejects(runRelease(config, 'deploy', { clock: () => 100, executor: async (program, args) => { if (args.includes('migrate')) migrated = true; return { code: 0, output: output(program, config) }; } }), /new verified backup/u); assert.equal(migrated, false);
  await assert.rejects(runRelease(config, 'deploy', { clock: () => 100, executor: async (program) => { if (program === config.backup.program) { await backup(config, 100, 2); await writeFile(join(config.backupDirectory, '20261008T000000Z-2.tar.age'), 'tampered'); } return { code: 0, output: output(program, config) }; } }), /checksum|size/u);
});
test('a concurrently replaced receipt cannot replace the backup actually verified for release', async (t) => {
  const config = await fixture(t); let verified;
  const record = await runRelease(config, 'deploy', { clock: () => 100, executor: async (program) => { if (program === config.backup.program) await backup(config, 100, 1); return { code: 0, output: output(program, config) }; }, verifyBackup: async (directory, now) => {
    verified = await verifiedBackupReceipt(directory, now);
    // A different writer publishes its receipt immediately after verification; its object is damaged.
    await backup(config, 100, 2); await writeFile(join(directory, '20261008T000000Z-2.tar.age'), 'unverified concurrent ciphertext');
    return verified;
  } });
  assert.ok(Object.isFrozen(verified)); assert.equal(JSON.parse(await readFile(join(config.backupDirectory, 'last-success.json'))).backup_name, '20261008T000000Z-2.tar.age');
  assert.deepEqual(record.backup, { name: verified.backup_name, sha256: verified.ciphertext_sha256, completedAt: verified.completed_at });
  assert.equal(record.backup.name, '20261008T000000Z-1.tar.age'); assert.deepEqual(JSON.parse(await readFile(join(config.stateDirectory, 'current.json'))).backup, record.backup);
  await assert.rejects(verifiedBackupReceipt(config.backupDirectory, 100), /size|checksum/u);
});
test('rollback requires matched schema and key compatibility and never runs migration', async (t) => {
  const config = await fixture(t); await mkdir(config.stateDirectory, { mode: 0o700 }); await writeFile(join(config.stateDirectory, 'current.json'), JSON.stringify({ release: 'new', rustImage: config.rustImage, edgeImage: config.edgeImage, previous: { release: 'old', rustImage: 'auth-rust-runtime:old', edgeImage: 'auth-rust-edge:old' } }), { mode: 0o600 });
  await assert.rejects(runRelease(config, 'rollback'), /compatibility/u);
  config.compatibilityFile = join(config.stateDirectory, 'compatibility.json'); await writeFile(config.compatibilityFile, JSON.stringify({ version: 1, from_release: 'new', to_release: 'old', schema_compatible: false, keys_compatible: true, reason: 'controlled incompatible schema' }), { mode: 0o600 }); await assert.rejects(runRelease(config, 'rollback'), /compatibility/u);
  await writeFile(config.compatibilityFile, JSON.stringify({ version: 1, from_release: 'new', to_release: 'old', schema_compatible: true, keys_compatible: true, reason: 'same reviewed schema and keys', old_rust_image: 'auth-rust-runtime:old', old_edge_image: 'auth-rust-edge:old', accepted_migration_sha256: [facts.migration_sha256], old_supported_kids: ['active'] }), { mode: 0o600 });
  let started = false;
  await assert.rejects(runRelease(config, 'rollback', { clock: () => 100, executor: async (program, args) => { if (args.includes('up')) started = true; return { code: 0, output: program === config.facts.program ? JSON.stringify({ ...facts, migration_sha256: 'b'.repeat(64) }) : '' }; } }), /exceed/u);
  await assert.rejects(runRelease(config, 'rollback', { clock: () => 100, executor: async (program, args) => { if (args.includes('up')) started = true; return { code: 0, output: program === config.facts.program ? JSON.stringify({ ...facts, used_kids: ['unsupported'], configured_kids: ['active', 'unsupported'] }) : '' }; } }), /exceed/u);
  assert.equal(started, false, 'Current actual migration or encrypted kid incompatibility must stop rollback before service replacement.');
  const visited = []; const record = await runRelease(config, 'rollback', { clock: () => 100, executor: async (program, args, options) => { visited.push(args); assert.equal(options.env.RUST_IMAGE, 'auth-rust-runtime:old'); if (program === config.backup.program) await backup(config, 100); return { code: 0, output: output(program, config) }; } });
  assert.equal(record.status, 'complete'); assert.equal(visited.some((args) => args.includes('migrate') || args.includes('down')), false); assert.equal(JSON.parse(await readFile(join(config.stateDirectory, 'current.json'))).release, 'old');
});
test('production rejects floating tags or partial config checks; local tags require acknowledgement', () => {
  const fields = { mode: 'production', project: 'identity-release', release: 'v1', rustImage: 'auth-rust-runtime:latest', edgeImage: 'auth-rust-edge:latest', composeFile: '/a/compose', envFile: '/a/env', stateDirectory: '/a/state', backupDirectory: '/a/backup', backup: { program: '/a/backup-command', args: [] }, smoke: { program: '/a/smoke', args: [] } };
  assert.throws(() => configuration(fields, 'production'), /immutable/u); assert.throws(() => configuration({ ...fields, mode: 'local-review' }), /acknowledged/u);
  assert.throws(() => configuration({ ...fields, rustImage: `registry/runtime@sha256:${'1'.repeat(64)}`, edgeImage: `registry/edge@sha256:${'2'.repeat(64)}`, configChecks: [] }, 'production'), /every Rust/u);
});
