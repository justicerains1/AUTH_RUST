import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import test from 'node:test';
import { validateBundle, validateCompose, deploy } from '../../infra/ops/deploy-production.mjs';

const root = resolve(import.meta.dirname, '../..');
const images = { runtime: `ghcr.io/example/auth/runtime@sha256:${'1'.repeat(64)}`, edge: `ghcr.io/example/auth/edge@sha256:${'2'.repeat(64)}` };
const model = { services: Object.fromEntries(['postgres', 'redis', 'api', 'worker', 'demo-a-bff', 'demo-b-bff', 'edge', 'migrate'].map((name) => [name, { image: name === 'edge' ? images.edge : ['postgres', 'redis'].includes(name) ? `registry.example/${name}@sha256:${'3'.repeat(64)}` : images.runtime }])) };
const digest = (value) => createHash('sha256').update(value).digest('hex');
async function fixture(t) {
  const directory = await mkdtemp(join(tmpdir(), 'identity-ci-deploy-')); t.after(() => rm(directory, { recursive: true, force: true })); const bundle = join(directory, 'bundle');
  execFileSync(process.execPath, [join(root, 'scripts/package-release.mjs'), bundle], { env: { ...process.env, RELEASE_REVISION: 'a'.repeat(40), RELEASE_REPOSITORY: 'example/auth', RELEASE_RUNTIME_IMAGE: images.runtime, RELEASE_EDGE_IMAGE: images.edge }, stdio: 'pipe' });
  const configPath = join(directory, 'deploy.json'); const backup = join(directory, 'backup'); await mkdir(backup, { mode: 0o700 }); const keys = join(directory, 'keys.json'); await writeFile(keys, JSON.stringify({ active: 'fixture-only-not-real-key' }), { mode: 0o600 });
  await writeFile(configPath, JSON.stringify({ project: 'identity-ci-deploy', envFile: join(directory, 'production.env'), stateDirectory: join(directory, 'state'), backupDirectory: backup, databaseUser: 'identity', databaseName: 'identity_production', encryptionKeysFile: keys, backup: { program: '/controlled/backup', args: [] }, smoke: { program: '/controlled/smoke', args: [] } }), { mode: 0o600 });
  return { directory, bundle, configPath, backup };
}
test('CI bundle contains deployment tools and verified age, excludes source and secrets', async (t) => {
  const f = await fixture(t); const m = await validateBundle(f.bundle); assert.deepEqual(m.images, images); assert.ok(m.files.some((file) => file.path === '.local/production/tools/age')); assert.equal(m.files.some((file) => /^(crates|apps|node_modules)\//u.test(file.path)), false); assert.equal(m.files.some((file) => /\.(pem|key)$/u.test(file.path)), false);
  const bytes = await readFile(`${f.bundle}.tar.gz`); assert.match(await readFile(`${f.bundle}.tar.gz.sha256`, 'utf8'), new RegExp(`^${digest(bytes)}  bundle.tar.gz\\n$`, 'u'));
  await writeFile(join(f.bundle, 'infra/ops/release.mjs'), 'tampered'); await assert.rejects(validateBundle(f.bundle), /checksum/u);
});
test('production rejects builds, floating images and mismatched CI digests', () => {
  assert.equal(validateCompose(model, { images }).length, 4);
  assert.throws(() => validateCompose({ services: { ...model.services, api: { image: images.runtime, build: '.' } } }, { images }), /build/u);
  assert.throws(() => validateCompose({ services: { ...model.services, redis: { image: 'redis:latest' } } }, { images }), /immutable/u);
  assert.throws(() => validateCompose({ services: { ...model.services, edge: { image: images.runtime } } }, { images }), /manifest/u);
});
test('plan validates bundle and Compose without pull, build, up or migration', async (t) => {
  const f = await fixture(t); const calls = []; const result = await deploy({ directory: f.bundle, configPath: f.configPath, action: 'plan', plan: true }, async (program, args) => { calls.push({ program, args }); return { code: 0, output: JSON.stringify(model) }; });
  assert.equal(result.plan, true); assert.equal(calls.length, 1); assert.ok(calls[0].args.includes('config')); assert.equal(calls.some((call) => call.args.includes('pull') || call.args.includes('build') || call.args.includes('up')), false);
});
test('failed image pull stops before data startup, backup or migration', async (t) => {
  const f = await fixture(t); const calls = [];
  await assert.rejects(deploy({ directory: f.bundle, configPath: f.configPath, action: 'deploy', plan: false }, async (_program, args) => { calls.push(args); return { code: args[0] === 'pull' ? 1 : 0, output: JSON.stringify(model) }; }), /pull/u);
  assert.equal(calls.some((args) => args.includes('up') || args.includes('migrate')), false);
});
test('actual verified backup then failed migration never starts application and all compose commands prohibit builds', async (t) => {
  const f = await fixture(t); const calls = []; const envs = [];
  await assert.rejects(deploy({ directory: f.bundle, configPath: f.configPath, action: 'deploy', plan: false }, async (program, args, options) => {
    calls.push({ program, args }); envs.push(options?.env);
    if (args.includes('--format')) return { code: 0, output: JSON.stringify(model) };
    if (program === '/controlled/backup') { const name = '20261009T000000Z-1.tar.age'; const bytes = Buffer.from('Synthetic encrypted receipt boundary only'); const sha = digest(bytes); await writeFile(join(f.backup, name), bytes, { mode: 0o600 }); await writeFile(join(f.backup, `${name}.sha256`), `${sha}  ${name}\n`, { mode: 0o600 }); await writeFile(join(f.backup, 'last-success.json'), JSON.stringify({ version: 1, completed_at: Math.floor(Date.now() / 1000), backup_name: name, ciphertext_bytes: bytes.length, ciphertext_sha256: sha }), { mode: 0o600 }); }
    if (args.includes('psql')) return { code: 0, output: 'absent\n' };
    return { code: args.includes('migrate') ? 7 : 0, output: '{}' };
  }), /migrate/u);
  const starts = calls.filter((call) => call.args.includes('up')); assert.equal(starts.length, 1); assert.ok(starts[0].args.includes('postgres')); assert.equal(calls.some((call) => call.program === '/controlled/smoke'), false);
  assert.ok(calls.filter((call) => call.program === 'docker' && (call.args.includes('up') || call.args.includes('run'))).every((call) => call.args.includes('--no-build'))); assert.equal(calls.some((call) => call.args.includes('build')), false); assert.ok(envs.some((env) => env?.RUST_IMAGE === images.runtime));
});
