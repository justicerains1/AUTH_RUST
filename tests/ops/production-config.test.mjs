// Offline Compose model only: no production secrets, image build, deployment or RPO claim.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import test from 'node:test';

const root = resolve(import.meta.dirname, '../..');
test('production PG archive command has required tool and independent storage mounts', (t) => {
  const fixture = mkdtempSync(join(tmpdir(), 'identity-compose-config-'));
  t.after(() => rmSync(fixture, { recursive: true, force: true }));
  const derived = readFileSync(resolve(root, 'infra/compose.prod.yaml'), 'utf8')
    .replaceAll(/env_file: \.\.\/\.local\/production\/(identity|demo-a|demo-b)\.env/gu,
      'env_file: [{ path: /deliberately-absent-production-env, required: false }]');
  const derivedPath = join(fixture, 'compose.prod.yaml'); writeFileSync(derivedPath, derived);
  const output = execFileSync('docker', ['compose', '--env-file', resolve(root, 'infra/production.env.example'),
    '--project-directory', resolve(root, 'infra'), '-f', derivedPath, 'config', '--no-env-resolution', '--format', 'json'], { encoding: 'utf8' });
  const config = JSON.parse(output);
  const postgres = config.services.postgres;
  assert.ok(postgres.command.includes('wal_level=replica'));
  assert.ok(postgres.command.includes('archive_mode=on'));
  assert.ok(postgres.command.includes('archive_timeout=300s'));
  assert.equal(postgres.command.at(-1), 'archive_command=/bin/sh /opt/identity-ops/archive-wal.sh "%p" "%f"');
  for (const target of ['/opt/identity-ops/archive-wal.sh', '/opt/identity-ops/age', '/var/lib/identity-backup/wal']) {
    const mount = postgres.volumes.find((volume) => volume.target === target);
    assert.ok(mount, target); assert.equal(mount.type, 'bind');
    // Compose omits the false default from canonical JSON; true would allow directory creation.
    assert.notEqual(mount.bind.create_host_path, true);
    if (target.startsWith('/opt/')) assert.equal(mount.read_only, true);
  }
  assert.equal(postgres.environment.WAL_ARCHIVE_DIRECTORY, '/var/lib/identity-backup/wal');
  assert.equal(postgres.environment.AGE_BINARY, '/opt/identity-ops/age');
  assert.equal(postgres.volumes.find((volume) => volume.target === '/opt/identity-ops/archive-wal.sh').source, resolve(root, 'infra/ops/archive-wal.sh'));
  assert.equal(postgres.volumes.find((volume) => volume.target === '/opt/identity-ops/age').source, resolve(root, '.local/production/tools/age'));
  assert.equal(postgres.ports, undefined);
  assert.equal(config.services.redis.ports, undefined);
  assert.deepEqual(config.services.edge.ports.map((port) => String(port.published)).sort(), ['443', '80']);
});
