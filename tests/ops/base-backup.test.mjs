// Actual encrypted PG backup file boundaries, isolated from every shared development dependency.
// AGE_BINARY / AGE_KEYGEN_BINARY select independently verified local age executables.
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, mkdirSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import test from 'node:test';

const root = resolve(import.meta.dirname, '../..');
const age = process.env.AGE_BINARY ?? 'age';
const ageKeygen = process.env.AGE_KEYGEN_BINARY ?? 'age-keygen';
const image = 'docker.m.daocloud.io/library/postgres:17.11-bookworm@sha256:3645570cccdfa447589da9f57dd740faa29b30938e861289a5574b6ca6b03826';

test('encrypted base backups restore only the selected relocated object', async (t) => {
  const fixture = mkdtempSync(join(tmpdir(), 'identity-backup-test-'));
  // PostgreSQL UID999 must traverse this synthetic mount; generated key/password files stay 0600.
  execFileSync('chmod', ['0755', fixture]);
  const container = `identity-backup-files-${randomUUID()}`;
  const docker = (...args) => execFileSync('docker', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
  t.after(() => {
    spawnSync('docker', ['rm', '-f', '-v', container], { stdio: 'ignore' });
    rmSync(fixture, { recursive: true, force: true });
  });
  // Missing age/Docker or image access is a real failure, never a skipped success.
  assert.match(execFileSync(age, ['--version'], { encoding: 'utf8' }), /1\.3\.2/u);
  const identity = join(fixture, 'identity.agekey');
  execFileSync(ageKeygen, ['-o', identity], { stdio: ['ignore', 'ignore', 'pipe'] });
  const recipient = execFileSync(ageKeygen, ['-y', identity], { encoding: 'utf8' }).trim();
  writeFileSync(join(fixture, 'pgpass'), 'localhost:*:*:postgres:fixture-only\n', { mode: 0o600 });
  mkdirSync(join(fixture, 'archive'), { mode: 0o777 });
  execFileSync('chmod', ['0777', join(fixture, 'archive')]);
  docker('run', '-d', '--network', 'none', '--name', container,
    '-e', 'POSTGRES_PASSWORD=fixture-only', '-e', `AGE_RECIPIENT=${recipient}`,
    '-e', 'AGE_BINARY=/opt/age', '-e', 'WAL_ARCHIVE_DIRECTORY=/fixture/archive',
    '-v', `${fixture}:/fixture`, '-v', `${join(root, 'infra/ops')}:/ops:ro`,
    '-v', `${resolve(age)}:/opt/age:ro`, image, 'postgres', '-c', 'wal_level=replica',
    '-c', 'archive_mode=on', '-c', 'archive_timeout=300s',
    '-c', 'archive_command=/bin/sh /ops/archive-wal.sh "%p" "%f"');
  let ready = false;
  for (let attempt = 0; attempt < 100; attempt += 1) {
    // TCP is only opened by the final server; initdb's temporary Unix socket is not readiness.
    if (spawnSync('docker', ['exec', container, 'pg_isready', '-h', '127.0.0.1', '-U', 'postgres'], { stdio: 'ignore' }).status === 0) { ready = true; break; }
    await delay(100);
  }
  assert.ok(ready, 'isolated PostgreSQL must become ready');
  docker('exec', container, 'psql', '-U', 'postgres', '-c', 'CREATE TABLE backup_probe(id integer PRIMARY KEY); INSERT INTO backup_probe VALUES (1);');
  await t.test('configured PG archive command publishes a real encrypted WAL segment', async () => {
    assert.equal(docker('exec', container, 'psql', '-U', 'postgres', '-Atc', 'SHOW archive_timeout').trim(), '5min');
    const wal = docker('exec', container, 'psql', '-U', 'postgres', '-Atc', 'SELECT pg_walfile_name(pg_current_wal_lsn())').trim();
    docker('exec', container, 'psql', '-U', 'postgres', '-Atc', 'SELECT pg_switch_wal()');
    let archived = false;
    for (let attempt = 0; attempt < 100; attempt += 1) {
      if (existsSync(join(fixture, 'archive', wal, 'wal.age'))) { archived = true; break; }
      await delay(100);
    }
    assert.ok(archived, 'a genuine WAL segment must be encrypted through archive_command');
    assert.ok(Number(docker('exec', container, 'psql', '-U', 'postgres', '-Atc', 'SELECT archived_count FROM pg_stat_archiver').trim()) > 0);
  });
  docker('exec', '-e', 'BACKUP_DESTINATION=/fixture/source', '-e', `AGE_RECIPIENT=${recipient}`,
    '-e', 'AGE_BINARY=/opt/age', '-e', 'PGPASSFILE=/fixture/pgpass', '-e', 'PGHOST=/var/run/postgresql', '-e', 'PGUSER=postgres',
    container, 'sh', '/ops/base-backup.sh');
  const originalDirectory = join(fixture, 'source/base');
  const name = readdirSync(originalDirectory).find((entry) => entry.endsWith('.tar.age'));
  assert.ok(name, 'base-backup.sh must publish an encrypted base backup');
  const original = join(originalDirectory, name);
  const originalManifest = readFileSync(`${original}.sha256`, 'utf8');
  assert.match(originalManifest, new RegExp(`^[a-f0-9]{64}  ${name.replaceAll('.', '\\.')}\\n$`, 'u'));
  const receiptFile = join(originalDirectory, 'last-success.json'); const receiptText = readFileSync(receiptFile, 'utf8'); const receipt = JSON.parse(receiptText);
  assert.deepEqual(Object.keys(receipt).sort(), ['backup_name', 'ciphertext_bytes', 'ciphertext_sha256', 'completed_at', 'version']);
  assert.equal(receipt.version, 1); assert.equal(receipt.backup_name, name); assert.equal(receipt.ciphertext_bytes, readFileSync(original).length);
  assert.equal(receipt.ciphertext_sha256, createHash('sha256').update(readFileSync(original)).digest('hex'));
  assert.ok(Number.isSafeInteger(receipt.completed_at) && Math.abs(receipt.completed_at - Date.now() / 1000) < 60);
  await t.test('a real failed backup preserves the last completed receipt', () => {
    const failed = spawnSync('docker', ['exec', '-e', 'BACKUP_DESTINATION=/fixture/source', '-e', `AGE_RECIPIENT=${recipient}`, '-e', 'AGE_BINARY=/opt/age', '-e', 'PGPASSFILE=/fixture/pgpass', '-e', 'PGHOST=/nonexistent', '-e', 'PGUSER=postgres', container, 'sh', '/ops/base-backup.sh'], { encoding: 'utf8' });
    assert.notEqual(failed.status, 0); assert.equal(readFileSync(receiptFile, 'utf8'), receiptText);
    assert.equal(readdirSync(originalDirectory).filter((entry) => entry.endsWith('.tar.age')).length, 1);
    assert.equal(readdirSync(originalDirectory).some((entry) => entry.startsWith('.base-backup.')), false);
  });
  const relocatedDirectory = join(fixture, 'relocated');
  renameSync(originalDirectory, relocatedDirectory);
  const backup = join(relocatedDirectory, name);
  const encrypted = readFileSync(backup);
  const digest = createHash('sha256').update(encrypted).digest('hex');
  const containerPath = (path) => `/fixture/${path.slice(fixture.length + 1)}`;
  const restore = (path, destination) => spawnSync('docker', ['exec', '-e', 'AGE_IDENTITY_FILE=/fixture/identity.agekey',
    '-e', 'AGE_BINARY=/opt/age', container, 'sh', '/ops/restore-base.sh', containerPath(path), containerPath(destination)], { encoding: 'utf8' });
  const assertFailed = (path, label) => {
    const destination = join(fixture, `failed-${label}`);
    const result = restore(path, destination);
    assert.notEqual(result.status, 0, label);
    assert.equal(existsSync(destination), false, `${label} must not publish PGDATA`);
  };
  await t.test('different mount path succeeds with the source absent', () => {
    const destination = join(fixture, 'restored-moved');
    const result = restore(backup, destination);
    assert.equal(result.status, 0, result.stderr);
    assert.ok(existsSync(join(destination, 'backup_manifest')));
    assert.equal(readFileSync(join(destination, 'PG_VERSION'), 'utf8').trim(), '17');
  });
  await t.test('an old path containing a different object is never read', () => {
    mkdirSync(originalDirectory, { recursive: true });
    writeFileSync(original, 'different old-path object');
    const destination = join(fixture, 'restored-old-object');
    const result = restore(backup, destination);
    assert.equal(result.status, 0, result.stderr);
    assert.ok(existsSync(join(destination, 'backup_manifest')));
  });
  await t.test('legacy path manifests are rejected until controlled conversion', () => {
    writeFileSync(`${backup}.sha256`, `${digest}  /fixture/source/base/${name}\n`);
    assertFailed(backup, 'legacy-path');
    writeFileSync(`${backup}.sha256`, originalManifest);
  });
  await t.test('corrupt selected ciphertext fails even with a valid old object', () => {
    writeFileSync(original, encrypted);
    const corrupted = Buffer.from(encrypted); corrupted[corrupted.length - 1] ^= 1;
    writeFileSync(backup, corrupted);
    assertFailed(backup, 'selected-corrupt');
    writeFileSync(backup, encrypted);
  });
  await t.test('missing, multi-record, wrong-name and wrong-hash manifests fail', () => {
    rmSync(`${backup}.sha256`); assertFailed(backup, 'missing-manifest');
    for (const [label, text] of [
      ['multiple-records', originalManifest + originalManifest],
      ['wrong-name', `${digest}  other.tar.age\n`],
      ['wrong-hash', `${'0'.repeat(64)}  ${name}\n`],
    ]) { writeFileSync(`${backup}.sha256`, text); assertFailed(backup, label); }
    writeFileSync(`${backup}.sha256`, originalManifest);
  });
  await t.test('authenticated encryption still rejects a tampered ciphertext with a changed checksum', () => {
    const corrupted = Buffer.from(encrypted); corrupted[corrupted.length - 1] ^= 1;
    writeFileSync(backup, corrupted);
    writeFileSync(`${backup}.sha256`, `${createHash('sha256').update(corrupted).digest('hex')}  ${name}\n`);
    assertFailed(backup, 'age-tag');
    writeFileSync(backup, encrypted); writeFileSync(`${backup}.sha256`, originalManifest);
  });
  await t.test('a valid encrypted tar with an invalid PG backup never publishes a destination', () => {
    const invalid = join(fixture, 'invalid.tar.age');
    const invalidSource = join(fixture, 'invalid-source'); mkdirSync(join(invalidSource, 'base'), { recursive: true });
    writeFileSync(join(invalidSource, 'base/PG_VERSION'), '17\n');
    execFileSync('tar', ['-C', invalidSource, '-cf', join(fixture, 'invalid.tar'), 'base']);
    execFileSync(age, ['-r', recipient, '-o', invalid, join(fixture, 'invalid.tar')]);
    const invalidHash = createHash('sha256').update(readFileSync(invalid)).digest('hex');
    writeFileSync(`${invalid}.sha256`, `${invalidHash}  ${basename(invalid)}\n`);
    assertFailed(invalid, 'invalid-pg-backup');
    assert.equal(readdirSync(dirname(invalid)).some((entry) => entry.startsWith('.base-restore.')), false);
  });
  await t.test('an existing restore destination and its sentinel remain untouched', () => {
    const destination = join(fixture, 'existing'); mkdirSync(destination); writeFileSync(join(destination, 'sentinel'), 'retained');
    assert.notEqual(restore(backup, destination).status, 0);
    assert.equal(readFileSync(join(destination, 'sentinel'), 'utf8'), 'retained');
  });
});
