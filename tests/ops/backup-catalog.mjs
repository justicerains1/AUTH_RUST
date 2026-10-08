// Real isolated PostgreSQL catalog extraction; actual current backups are held, never aged artificially.
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { chmod, chown, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { applyRetention, planRetention } from '../../infra/ops/backup-retention.mjs';

const root = resolve(import.meta.dirname, '../..');
const directory = join(tmpdir(), `identity-real-catalog-${randomUUID()}`);
const pgImage = 'docker.m.daocloud.io/library/postgres:17.11-bookworm@sha256:3645570cccdfa447589da9f57dd740faa29b30938e861289a5574b6ca6b03826';
const nodeImage = 'docker.m.daocloud.io/library/node:22.22.1-bookworm-slim@sha256:4f77a690f2f8946ab16fe1e791a3ac0667ae1c3575c3e4d0d4589e9ed5bfaf3d';
const age = process.env.AGE_BINARY ?? resolve(root, '.local/security-tools/age/age');
const keygen = process.env.AGE_KEYGEN_BINARY ?? resolve(root, '.local/security-tools/age/age-keygen');
const containers = [];
const report = { started: new Date().toISOString(), scope: 'Local actual pg_basebackup/age/pg_verifybackup catalog and restored probe; no fourteen-day history, no real deletion or production RPO', checks: [], sourceSha256: {}, exitCode: 1 };
function run(program, args) { return execFileSync(program, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }); }
const docker = (...args) => run('docker', args);
function own(prefix) { const name = `${prefix}-${randomUUID()}`; containers.push(name); return name; }
async function ready(name) {
  for (let i = 0; i < 100; i += 1) { try { if (docker('exec', name, 'pg_isready', '-h', '127.0.0.1', '-U', 'postgres').includes('accepting connections')) return; } catch { /* Actual owned startup only. */ } await delay(100); }
  throw new Error('Owned PostgreSQL did not start.');
}
try {
  await mkdir(directory, { mode: 0o700 }); await mkdir(join(directory, 'work'), { mode: 0o700 });
  run(keygen, ['-o', join(directory, 'backup.agekey')]); const recipient = run(keygen, ['-y', join(directory, 'backup.agekey')]).trim();
  assert.match(run(age, ['--version']), /1\.3\.2/u);
  const nodeExtractor = own('identity-catalog-node');
  docker('create', '--name', nodeExtractor, nodeImage);
  docker('cp', `${nodeExtractor}:/usr/local/bin/node`, join(directory, 'node'));
  docker('rm', nodeExtractor); containers.splice(containers.indexOf(nodeExtractor), 1);
  await chmod(join(directory, 'node'), 0o755);
  const primary = own('identity-catalog-primary');
  docker('run', '-d', '--network', 'none', '--name', primary, '-e', 'POSTGRES_HOST_AUTH_METHOD=trust', '-v', `${directory}:${directory}`, '-v', `${root}:${root}:ro`, '-v', `${age}:/opt/age:ro`, pgImage);
  await ready(primary);
  docker('exec', primary, 'psql', '-U', 'postgres', '-c', 'CREATE TABLE catalog_probe(id integer PRIMARY KEY); INSERT INTO catalog_probe VALUES(1);');
  await writeFile(join(directory, 'pgpass'), '', { mode: 0o600 });
  const backupDirectory = join(directory, 'base');
  const backup = () => docker('exec', '-e', `BACKUP_DIRECTORY=${backupDirectory}`, '-e', `AGE_RECIPIENT=${recipient}`, '-e', 'AGE_BINARY=/opt/age', '-e', `PGPASSFILE=${join(directory, 'pgpass')}`, '-e', 'PGHOST=/var/run/postgresql', '-e', 'PGUSER=postgres', primary, 'sh', resolve(root, 'infra/ops/base-backup.sh'));
  const config = { backupDirectory, workDirectory: join(directory, 'work'), restoreScript: resolve(root, 'infra/ops/restore-base.sh'), ageIdentityFile: join(directory, 'backup.agekey'), ageBinary: '/opt/age', pgControlData: '/usr/lib/postgresql/17/bin/pg_controldata', keepDays: 14, exclusiveMaintenanceAcknowledged: true };
  const configPath = join(directory, 'catalog-config.json'); await writeFile(configPath, JSON.stringify(config), { mode: 0o600 });
  const catalogs = [];
  for (let index = 0; index < 2; index += 1) {
    backup();
    docker('exec', primary, join(directory, 'node'), resolve(root, 'infra/ops/backup-retention.mjs'), 'catalog', configPath);
    const receipt = JSON.parse(await readFile(join(backupDirectory, 'last-success.json'), 'utf8'));
    const path = join(backupDirectory, `${receipt.backup_name}.catalog.json`); const value = JSON.parse(await readFile(path, 'utf8'));
    const id = docker('exec', primary, 'psql', '-U', 'postgres', '-Atc', 'SELECT system_identifier FROM pg_control_system()').trim();
    assert.equal(value.system_identifier, id); assert.equal(value.wal_ranges[0].timeline, 1); assert.equal(value.wal_segment_bytes, 16777216); assert.equal(value.ciphertext_sha256, receipt.ciphertext_sha256);
    assert.ok(value.completed_at >= value.started_at); assert.ok(value.verified_at >= value.completed_at);
    const permissions = docker('exec', primary, 'stat', '-c', '%a', path).trim(); assert.equal(permissions, '600');
    catalogs.push(value);
    if (index === 0) docker('exec', primary, 'psql', '-U', 'postgres', '-c', 'INSERT INTO catalog_probe VALUES(2);');
  }
  assert.notEqual(catalogs[0].backup_name, catalogs[1].backup_name);
  report.checks.push('Two actual encrypted PostgreSQL backups yield owner-only catalogs bound to ciphertext and pg_verifybackup manifest metadata.');
  const latest = catalogs[1];
  // Real startup/restored business probe proves this particular object, without invented historical time.
  const restoredPath = join(directory, 'restored');
  docker('exec', '-e', `AGE_IDENTITY_FILE=${join(directory, 'backup.agekey')}`, '-e', 'AGE_BINARY=/opt/age', primary, 'sh', config.restoreScript, join(backupDirectory, latest.backup_name), restoredPath);
  await chown(restoredPath, 999, 999);
  docker('exec', primary, 'chown', '-R', '999:999', restoredPath);
  const recovered = own('identity-catalog-recovered');
  docker('run', '-d', '--network', 'none', '--name', recovered, '-v', `${restoredPath}:/var/lib/postgresql/data`, pgImage);
  await ready(recovered);
  assert.equal(docker('exec', recovered, 'psql', '-U', 'postgres', '-Atc', 'SELECT count(*) FROM catalog_probe').trim(), '2');
  const systemId = docker('exec', recovered, 'psql', '-U', 'postgres', '-Atc', 'SELECT system_identifier FROM pg_control_system()').trim();
  const timeline = Number(docker('exec', recovered, 'psql', '-U', 'postgres', '-Atc', 'SELECT timeline_id FROM pg_control_checkpoint()').trim());
  const replayed = docker('exec', recovered, 'psql', '-U', 'postgres', '-Atc', 'SELECT pg_current_wal_lsn()').trim();
  assert.equal(systemId, latest.system_identifier); assert.equal(timeline, latest.wal_ranges[0].timeline);
  const verificationTime = Math.floor(Date.now() / 1000);
  const proof = { version: 1, backup_name: latest.backup_name, ciphertext_sha256: latest.ciphertext_sha256, manifest_sha256: latest.manifest_sha256, system_identifier: systemId, restore_verified: true, pg_verifybackup_passed: true, recovered_timeline: timeline, replayed_lsn: replayed, verification_completed_at: verificationTime, recovery_target_time: latest.completed_at, checks: ['postgres-recovery', 'business-validation'] };
  await writeFile(join(backupDirectory, `${latest.backup_name}.restore-proof.json`), JSON.stringify(proof), { mode: 0o600 });
  report.checks.push('Selected latest ciphertext really restores to the same cluster/timeline and both actual committed probe rows; LSN/time proof remains bound to that object.');
  // This fresh real inventory cannot contain an anchor fourteen days ago. It must hold everything.
  const plan = await planRetention({ ...config, ageBinary: age }, verificationTime);
  assert.equal(plan.hold, 'no-complete-anchor-before-window'); assert.deepEqual(plan.candidates, []);
  await assert.rejects(applyRetention({ ...config, ageBinary: age }, { now: verificationTime, reviewedPlanSha256: createHash('sha256').update(JSON.stringify(plan)).digest('hex') }), /held/u);
  assert.equal((await readdir(backupDirectory)).filter((name) => name.endsWith('.tar.age')).length, 2);
  report.checks.push('With no real fourteen-day history the actual catalog inventory holds every backup and apply refuses; no timestamps or proofs are aged to obtain deletion.');
  report.catalogCount = 2; report.restoreProofCreated = true; report.actualApply = 'refused-no-fourteen-day-anchor';
  for (const file of ['infra/ops/backup-retention.mjs', 'tests/ops/backup-catalog.mjs', 'infra/ops/base-backup.sh', 'infra/ops/restore-base.sh']) report.sourceSha256[file] = createHash('sha256').update(await readFile(resolve(root, file))).digest('hex');
  report.exitCode = 0; console.log('Real PostgreSQL backup catalog and recovery proof passed; current backups held and apply refused without fourteen-day history.');
} catch (error) {
  report.failure = error.message; process.exitCode = 1;
  console.error('Real backup catalog verification failed; no retention success or deletion claimed.');
} finally {
  for (const name of containers) { try { docker('rm', '-f', '-v', name); } catch { report.cleanupFailure = true; report.exitCode = 1; process.exitCode = 1; } }
  await rm(directory, { recursive: true, force: true }); report.completed = new Date().toISOString();
  await mkdir(resolve(root, 'docs/evidence/T22'), { recursive: true });
  await writeFile(resolve(root, `docs/evidence/T22/backup-catalog-${Date.now()}.json`), JSON.stringify(report, null, 2) + '\n');
}
