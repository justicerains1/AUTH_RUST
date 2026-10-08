// Real, isolated local container drill. Never a production deployment or a cross-version guarantee.
import assert from 'node:assert/strict';
import { createHash, randomBytes, randomUUID } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { chown, chmod, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { createServer } from 'node:net';
import { configuration, databaseFacts, executeCommand, runRelease } from '../../infra/ops/release.mjs';

const root = resolve(import.meta.dirname, '../..');
const image = 'docker.m.daocloud.io/library/postgres:17.11-bookworm@sha256:3645570cccdfa447589da9f57dd740faa29b30938e861289a5574b6ca6b03826';
const directory = process.env.RELEASE_DRILL_DIRECTORY ?? resolve(root, `.local/release-drill-${randomBytes(8).toString('hex')}`);
const keyFile = resolve(directory, 'keys.json');
const metadata = resolve(directory, 'fixture.json');
const run = (program, args) => execFileSync(program, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
const docker = (...args) => run('docker', args);
const hash = (value) => createHash('sha256').update(value).digest('hex');
async function facts(fixture) {
  return databaseFacts({ project: fixture.project, composeFile: resolve(directory, 'compose.yaml'), envFile: resolve(directory, 'release.env'), databaseUser: 'postgres', databaseName: 'identity_test', encryptionKeysFile: keyFile, rustImage: process.env.RUST_IMAGE ?? 'auth-rust-runtime:t22-review', edgeImage: 'auth-rust-edge:t23-1c0c9ea' });
}
async function smoke(fixture) {
  // This smoke executes in the owned internal Docker network; no host ports or external TLS.
  const base = 'http://api:8080'; const origin = `http://localhost:${fixture.port}`;
  const preauth = await fetch(`${base}/api/v1/auth/csrf`); assert.equal(preauth.status, 200);
  let cookie = preauth.headers.get('set-cookie').split(';')[0]; let csrf = (await preauth.json()).csrf_token;
  const response = await fetch(`${base}/api/v1/auth/login/password`, { method: 'POST', headers: { origin, cookie, 'x-csrf-token': csrf, 'content-type': 'application/json' }, body: JSON.stringify({ email: fixture.email, password: fixture.password }) }); assert.equal(response.status, 200);
  const logged = await response.json(); assert.equal(logged.status, 'authenticated'); csrf = logged.csrf_token; cookie = response.headers.getSetCookie().filter((value) => !value.includes('Max-Age=0')).map((value) => value.split(';')[0]).join('; ');
  // Bootstrap-only users intentionally cannot access ordinary account APIs. Prove their session
  // against its actual bounded /me authority, then retire it, without fabricating privileges.
  const me = await fetch(`${base}/api/v1/me`, { headers: { cookie } }); assert.equal(me.status, 200);
  assert.equal((await fetch(`${base}/api/v1/auth/logout`, { method: 'POST', headers: { origin, cookie, 'x-csrf-token': csrf, 'content-type': 'application/json' }, body: '{}' })).status, 204);
  assert.equal((await fetch(`${base}/api/v1/me`, { headers: { cookie } })).status, 401);
}
if (process.argv[2]) {
  try { const fixture = JSON.parse(await readFile(metadata, 'utf8')); if (process.argv[2] === '--facts') console.log(JSON.stringify(await facts(fixture))); else if (process.argv[2] === '--smoke') { await smoke(fixture); console.log('Actual password/session/logout smoke passed.'); } else throw new Error('Unknown isolated drill command.'); }
  catch (error) { console.error(`Isolated release facts/smoke failed: ${error.message}; ${error.cause?.message ?? 'no transport cause'}`); process.exitCode = 1; }
} else {
  const report = { started: new Date().toISOString(), scope: 'Actual isolated local release/backup/migration failure/compatible rollback; no production TLS/SMTP/HA', phases: [], exitCode: 1 };
  const postgres = `release-pg-${randomUUID()}`; const project = `release-drill-${randomBytes(6).toString('hex')}`; let fixture; let config; let helper;
  try {
    await mkdir(directory, { recursive: true, mode: 0o700 }); await mkdir(resolve(directory, 'secret'), { mode: 0o700 });
    const server = createServer(); await new Promise((done) => server.listen(0, '127.0.0.1', done)); const port = server.address().port; await new Promise((done) => server.close(done));
    const signing = resolve(directory, 'secret/signing.pem'); run('openssl', ['genpkey', '-algorithm', 'RSA', '-pkeyopt', 'rsa_keygen_bits:2048', '-out', signing]); await chown(signing, 10001, 10001); await chmod(signing, 0o600);
    // Container UID10001 must traverse only the synthetic secret mount parent.
    await chmod(resolve(directory, 'secret'), 0o755);
    const aead = randomBytes(32).toString('base64'); await writeFile(keyFile, JSON.stringify({ 'drill-key': aead }), { mode: 0o600 }); await chown(keyFile, 10001, 10001);
    const age = process.env.AGE_BINARY ?? resolve(root, '.local/security-tools/age/age'); const keygen = process.env.AGE_KEYGEN_BINARY ?? resolve(root, '.local/security-tools/age/age-keygen'); run(keygen, ['-o', resolve(directory, 'backup.agekey')]); const recipient = run(keygen, ['-y', resolve(directory, 'backup.agekey')]).trim();
    const compose = `name: ${project}\nservices:\n  postgres:\n    image: ${image}\n    environment: { POSTGRES_HOST_AUTH_METHOD: trust, POSTGRES_DB: identity_test }\n    networks: [private]\n    healthcheck: { test: [CMD, pg_isready, -U, postgres], interval: 1s, timeout: 1s, retries: 30 }\n  redis:\n    image: docker.m.daocloud.io/library/redis:7.4@sha256:4fa24486b8bcca8eec45ee0eb166edc674795e53a2b53d1a9ef263eecebaac85\n    networks: [private]\n    healthcheck: { test: [CMD, redis-cli, ping], interval: 1s, timeout: 1s, retries: 30 }\n  api:\n    image: __RUST_IMAGE__\n    user: '10001:10001'\n    read_only: true\n    cap_drop: [ALL]\n    security_opt: [no-new-privileges:true]\n    tmpfs: ['/tmp:size=16m,mode=1777']\n    command: [identity-server]\n    env_file: ${resolve(directory, 'identity.env')}\n    volumes: ['${signing}:/run/secrets/signing.pem:ro', '${keyFile}:/run/secrets/keys.json:ro']\n    networks: [private]\n    healthcheck: { test: [CMD, curl, --fail, --silent, http://127.0.0.1:8080/health/ready], interval: 1s, timeout: 2s, retries: 30 }\n  edge:\n    image: __EDGE_IMAGE__\n    user: '10001:10001'\n    read_only: true\n    cap_drop: [ALL]\n    security_opt: [no-new-privileges:true]\n    command: [caddy, validate, --config, /etc/caddy/Caddyfile, --adapter, caddyfile]\n    environment: { IDENTITY_HOST: identity.example, DEMO_A_HOST: app-a.example, DEMO_B_HOST: app-b.example, TLS_EMAIL: operations@example }\n    networks: [private]\n  migrate:\n    image: __RUST_IMAGE__\n    profiles: [maintenance]\n    environment: { APP_ENV: test, DATABASE_URL: 'postgres://postgres@postgres/identity_test' }\n    command: [identity-migrate, --test-target]\n    networks: [private]\nnetworks:\n  private: { internal: true }\n`;
    const composeFile = resolve(directory, 'compose.yaml'); await writeFile(composeFile, compose.replaceAll('__RUST_IMAGE__', '${RUST_IMAGE}').replaceAll('__EDGE_IMAGE__', '${EDGE_IMAGE}'), { mode: 0o600 });
    await writeFile(resolve(directory, 'identity.env'), `APP_ENV=test\nBIND=0.0.0.0:8080\nISSUER=http://localhost:${port}\nRP_ID=localhost\nDATABASE_URL=postgres://postgres@postgres/identity_test\nREDIS_URL=redis://redis:6379\nSIGNING_KEY_FILE=/run/secrets/signing.pem\nSIGNING_KID=drill-signing\nENCRYPTION_KEYS_FILE=/run/secrets/keys.json\nACTIVE_ENCRYPTION_KID=drill-key\nSMTP_HOST=127.0.0.1\nSMTP_PORT=1025\nSMTP_FROM=no-reply@localhost\nSMTP_TLS=disabled\n`, { mode: 0o600 });
    await writeFile(resolve(directory, 'release.env'), 'RUST_IMAGE=auth-rust-runtime:t22-review\nEDGE_IMAGE=auth-rust-edge:t23-1c0c9ea\n', { mode: 0o600 });
    const ca = ['compose', '--project-name', project, '--env-file', resolve(directory, 'release.env'), '-f', composeFile];
    docker(...ca, 'up', '-d', '--wait', 'postgres', 'redis');
    // Backup only the actual owned Compose database via its container.
    const actualPg = docker(...ca, 'ps', '-q', 'postgres').trim();
    docker('cp', age, `${actualPg}:/opt/age`); docker('exec', actualPg, 'chmod', '755', '/opt/age'); docker('cp', resolve(root, 'infra/ops/base-backup.sh'), `${actualPg}:/opt/base-backup.sh`);
    // Attach only this owned DB's backup directory; docker cp below retrieves completed encrypted objects.
    const backupScript = resolve(directory, 'backup.mjs');
    await writeFile(backupScript, `import{execFileSync}from'node:child_process';import{mkdirSync}from'node:fs';const d=${JSON.stringify(resolve(directory, 'backup'))};mkdirSync(d,{recursive:true,mode:0o700});execFileSync('docker',['exec','-e','BACKUP_DIRECTORY=/tmp/release-backup','-e',${JSON.stringify(`AGE_RECIPIENT=${recipient}`)},'-e','AGE_BINARY=/opt/age','-e','PGPASSFILE=/tmp/empty-pgpass','-e','PGHOST=/var/run/postgresql','-e','PGUSER=postgres',${JSON.stringify(actualPg)},'sh','-c','touch /tmp/empty-pgpass && chmod 600 /tmp/empty-pgpass && sh /opt/base-backup.sh'],{stdio:'pipe'});execFileSync('docker',['cp',${JSON.stringify(actualPg + ':/tmp/release-backup/.')},d],{stdio:'pipe'});\n`, { mode: 0o600 });
    fixture = { port, project, postgres: actualPg, email: 'release-drill@example.test', password: `Local release ${randomBytes(32).toString('base64url')}` }; await writeFile(metadata, JSON.stringify(fixture), { mode: 0o600 });
    const envBase = { RELEASE_DRILL_DIRECTORY: directory };
    helper = async (program, args, options) => executeCommand(program, args, { ...options, env: { ...options.env, ...envBase } });
    const nodeImage = 'docker.m.daocloud.io/library/node:22.22.1-bookworm-slim@sha256:4f77a690f2f8946ab16fe1e791a3ac0667ae1c3575c3e4d0d4589e9ed5bfaf3d';
    const base = { mode: 'local-review', project, release: 'old', rustImage: 'auth-rust-runtime:t22-review', edgeImage: 'auth-rust-edge:t23-1c0c9ea', composeFile, envFile: resolve(directory, 'release.env'), stateDirectory: resolve(directory, 'state'), backupDirectory: resolve(directory, 'backup'), services: ['api'], configChecks: [['api', 'identity-server']], backup: { program: process.execPath, args: [backupScript] }, facts: { program: process.execPath, args: [resolve(root, 'tests/ops/release-local.mjs'), '--facts'] }, smoke: { program: '/usr/bin/docker', args: ['run', '--rm', '--network', `${project}_private`, '-e', `RELEASE_DRILL_DIRECTORY=${directory}`, '-v', `${root}:${root}:ro`, nodeImage, 'node', resolve(root, 'tests/ops/release-local.mjs'), '--smoke'] } };
    config = configuration(base, 'local-review');
    // First migration is an explicit preparatory operation, solely to seed a user for real smoke.
    docker(...ca, '--profile', 'maintenance', 'run', '--rm', '--no-deps', 'migrate');
    // The actual stdin-only bootstrap CLI hashes and creates the first verified identity.
    // It remains binding-only; password/session smoke does not use administrator privileges.
    const seeded = await executeCommand('docker', [...ca, 'run', '--rm', '--no-deps', '-T', 'api', 'identity-admin-cli', 'bootstrap', '--email', fixture.email], { env: { RUST_IMAGE: base.rustImage, EDGE_IMAGE: base.edgeImage }, input: `${fixture.password}\n` });
    await writeFile(resolve(directory, 'bootstrap-diagnostic.txt'), seeded.output, { mode: 0o600 });
    if (seeded.code !== 0) report.bootstrapSafeDiagnostic = seeded.output.replaceAll(fixture.password, '[PASSWORD]');
    assert.equal(seeded.code, 0, 'Actual owner-only bootstrap failed.');
    report.phases.push('Owned PostgreSQL/Redis started privately; actual CLI created a verified fixture identity without granting full admin access.');
    const first = await runRelease(config, 'deploy', { executor: helper });
    report.phases.push({ action: 'first-release', status: first.status, stages: first.stages, images: first.images, backup: first.backup });
    const secondConfig = configuration({ ...base, release: 'new', rustImage: 'auth-rust-runtime:t23-1c0c9ea' }, 'local-review');
    const second = await runRelease(secondConfig, 'deploy', { executor: helper });
    report.phases.push({ action: 'second-release', status: second.status, stages: second.stages, images: second.images, backup: second.backup });
    const actualBeforeFailure = docker(...ca, 'ps', '-q', 'api').trim();
    const beforeHistory = (await facts(fixture)).migration_sha256;
    // Deliberately incompatible SQLx history in this isolated fixture makes the actual migrator fail.
    const originalChecksum = docker('exec', actualPg, 'psql', '-U', 'postgres', '-d', 'identity_test', '-Atc', "SELECT encode(checksum,'hex') FROM _sqlx_migrations WHERE version=(SELECT min(version) FROM _sqlx_migrations)").trim();
    docker('exec', actualPg, 'psql', '-U', 'postgres', '-d', 'identity_test', '-c', "UPDATE _sqlx_migrations SET checksum=decode(repeat('00',48),'hex') WHERE version=(SELECT min(version) FROM _sqlx_migrations)");
    let migrationFailed = false;
    try { await runRelease(configuration({ ...base, release: 'must-not-start', rustImage: secondConfig.rustImage }, 'local-review'), 'deploy', { executor: helper }); }
    catch (error) { assert.match(error.message, /migrate/u); migrationFailed = true; }
    assert.ok(migrationFailed); assert.equal(docker(...ca, 'ps', '-q', 'api').trim(), actualBeforeFailure, 'Failed migration must never replace the running API container.');
    const unchangedState = JSON.parse(await readFile(resolve(directory, 'state/current.json'), 'utf8')); assert.equal(unchangedState.release, 'new');
    docker('exec', actualPg, 'psql', '-U', 'postgres', '-d', 'identity_test', '-c', `UPDATE _sqlx_migrations SET checksum=decode('${originalChecksum}','hex') WHERE version=(SELECT min(version) FROM _sqlx_migrations)`);
    assert.equal((await facts(fixture)).migration_sha256, beforeHistory);
    report.phases.push('Actual checksum-invalid SQLx migration failed; no up/ready/smoke ran, original API container and recorded new release remained unchanged.');
    const compatibilityFile = resolve(directory, 'compatibility.json');
    await writeFile(compatibilityFile, JSON.stringify({ version: 1, from_release: 'new', to_release: 'old', schema_compatible: true, keys_compatible: true, reason: 'Recorded older runtime supports this unchanged SQLx migration history and identical AEAD key map in the isolated fixture.', old_rust_image: base.rustImage, old_edge_image: base.edgeImage, accepted_migration_sha256: [beforeHistory], old_supported_kids: ['drill-key'] }), { mode: 0o600 });
    const rolled = await runRelease(configuration({ ...secondConfig, compatibilityFile }, 'local-review'), 'rollback', { executor: helper });
    assert.equal(rolled.status, 'complete'); assert.equal(rolled.stages.some((item) => item.stage === 'migrate'), false);
    assert.equal(JSON.parse(await readFile(resolve(directory, 'state/current.json'), 'utf8')).release, 'old');
    report.phases.push({ action: 'compatible-rollback', status: rolled.status, stages: rolled.stages, images: rolled.images, backup: rolled.backup, compatibilitySha256: rolled.compatibilitySha256 });
    report.schemaMigrationSha256 = beforeHistory;
    report.sourceSha256 = {}; for (const file of ['infra/ops/release.mjs', 'tests/ops/release-local.mjs', 'infra/ops/base-backup.sh']) report.sourceSha256[file] = hash(await readFile(resolve(root, file)));
    report.commit = run('git', ['-C', root, 'rev-parse', 'HEAD']).trim();
    report.exitCode = 0; console.log('Isolated real release, verified backup, migration-stop and compatible image rollback smoke passed.');
  } catch (error) {
    report.failure = error.message; process.exitCode = 1;
    if (config) {
      const events = (await readdir(config.stateDirectory).catch(() => [])).filter((file) => file.endsWith('.json'));
      for (const file of events) { const value = JSON.parse(await readFile(resolve(config.stateDirectory, file), 'utf8')); if (value.status === 'failed') report.failedStages = value.stages; }
      const diagnostics = (await readdir(config.stateDirectory).catch(() => [])).filter((file) => file.endsWith('.diagnostic.txt'));
      const texts = await Promise.all(diagnostics.map((file) => readFile(resolve(config.stateDirectory, file), 'utf8')));
      report.safeDiagnostic = texts.filter((text) => text.includes('failed') || text.includes('ERR_ASSERTION')).join('\n').replaceAll(fixture?.password ?? 'UNUSED', '[PASSWORD]');
    }
    console.error('Isolated release drill failed; private diagnostics retained only locally.');
  }
  finally {
    if (fixture) { try { await writeFile(resolve(directory, 'latest-schema-facts.json'), JSON.stringify(await facts(fixture)), { mode: 0o600 }); } catch { /* Owned DB may already be unavailable on setup failure. */ } }
    if (config) { try { docker('compose', '--project-name', project, '--env-file', config.envFile, '-f', config.composeFile, 'down', '--volumes'); } catch { report.cleanupFailure = true; report.exitCode = 1; process.exitCode = 1; } }
    else { try { docker('rm', '-f', '-v', postgres); } catch { /* Only the owned preliminary container. */ } }
    await rm(directory, { recursive: true, force: true }); report.completed = new Date().toISOString();
    await mkdir(resolve(root, 'docs/evidence/T22'), { recursive: true }); await writeFile(resolve(root, `docs/evidence/T22/release-local-${Date.now()}.json`), JSON.stringify(report, null, 2) + '\n');
  }
}
