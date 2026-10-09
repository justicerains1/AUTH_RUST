// CI produces a deployment-only bundle. No source tree, credentials or build tools are shipped.
import { mkdir, readFile, copyFile, writeFile } from 'node:fs/promises';
import { resolve, dirname } from 'node:path';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';

const root = resolve(import.meta.dirname, '..');
const output = resolve(process.argv[2] ?? resolve(root, '.local/release-bundle'));
const revision = process.env.RELEASE_REVISION;
const repository = process.env.RELEASE_REPOSITORY?.toLowerCase();
const images = { runtime: process.env.RELEASE_RUNTIME_IMAGE, edge: process.env.RELEASE_EDGE_IMAGE };
if (!/^[a-f0-9]{40}$/u.test(revision ?? '') || !/^[a-z0-9_.-]+\/[a-z0-9_.-]+$/u.test(repository ?? '') || Object.values(images).some((image) => !/^ghcr\.io\/[a-z0-9_./-]+@sha256:[a-f0-9]{64}$/u.test(image ?? ''))) throw new Error('CI revision, repository and immutable image digests are required.');
const files = ['install.sh', 'infra/ops/install-production.mjs', 'infra/ops/installer-operations.mjs', 'infra/compose.prod.yaml', 'infra/production.env.example', 'infra/ops/release.mjs', 'infra/ops/collect-metrics.mjs', 'infra/ops/archive-wal.sh', 'infra/ops/restore-wal.sh', 'infra/ops/base-backup.sh', 'infra/ops/restore-base.sh', 'infra/ops/backup-retention.mjs', 'infra/ops/identity-alerts.yaml', 'infra/ops/prometheus.yaml', 'infra/ops/metrics.env.example', 'infra/ops/alertmanager.yaml.example', 'infra/ops/systemd/identity-base-backup.service', 'infra/ops/systemd/identity-base-backup.timer', 'infra/ops/systemd/identity-ops-metrics.service', 'infra/ops/systemd/identity-ops-metrics.timer', 'infra/ops/deploy-production.mjs', 'infra/ops/deploy-production.sh', 'infra/ops/release-config.example.json', 'docs/runbooks/production-ci-deployment.md'];
await mkdir(output, { recursive: true });
const manifest = { version: 1, repository, revision, platform: 'linux/amd64', images, files: [] };
for (const path of files) { const bytes = await readFile(resolve(root, path)); const target = resolve(output, path); await mkdir(dirname(target), { recursive: true }); await copyFile(resolve(root, path), target); manifest.files.push({ path, sha256: createHash('sha256').update(bytes).digest('hex') }); }
// The PG archive container needs age at this pre-reviewed mount path; ship the verified executable.
const agePath = '.local/production/tools/age'; const age = await readFile(resolve(root, '.local/security-tools/age/age'));
if (createHash('sha256').update(age).digest('hex') !== 'eb7dd1b518f0a307c99cd97782623c5321da049154b04acd2d98d21aa7bc9b2c') throw new Error('Pinned age binary checksum differs.');
await mkdir(dirname(resolve(output, agePath)), { recursive: true }); await writeFile(resolve(output, agePath), age, { mode: 0o755 }); manifest.files.push({ path: agePath, sha256: createHash('sha256').update(age).digest('hex') });
await writeFile(resolve(output, 'release-manifest.json'), JSON.stringify(manifest, null, 2) + '\n');
execFileSync('tar', ['-czf', `${output}.tar.gz`, '-C', output, '.']);
const digest = createHash('sha256').update(await readFile(`${output}.tar.gz`)).digest('hex');
await writeFile(`${output}.tar.gz.sha256`, `${digest}  ${output.split('/').at(-1)}.tar.gz\n`);
console.log('Deployment-only release bundle and SHA256 manifest generated.');
