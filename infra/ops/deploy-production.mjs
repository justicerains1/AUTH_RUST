// Deploy verified CI artifacts; compose build directives are rejected before any mutation.
import { constants } from 'node:fs';
import { open, readFile, mkdir, stat, lstat, realpath, rm } from 'node:fs/promises';
import { resolve, dirname, isAbsolute, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { configuration, databaseFacts, executeCommand, runRelease } from './release.mjs';

const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');
export async function validateBundle(directory) {
  if (!isAbsolute(directory) || await realpath(directory) !== directory) throw new Error('A canonical extracted release directory is required.');
  const manifestPath = join(directory, 'release-manifest.json');
  if ((await lstat(manifestPath)).isSymbolicLink()) throw new Error('Release manifest cannot be a symbolic link.');
  const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
  if (manifest.version !== 1 || manifest.platform !== 'linux/amd64' || !/^[a-f0-9]{40}$/u.test(manifest.revision ?? '') || !Array.isArray(manifest.files) || !manifest.files.length || new Set(manifest.files.map((file) => file.path)).size !== manifest.files.length || !manifest.images || JSON.stringify(Object.keys(manifest.images).sort()) !== JSON.stringify(['edge', 'runtime']) || Object.values(manifest.images).some((value) => !/^ghcr\.io\/[a-z0-9_./-]+@sha256:[a-f0-9]{64}$/u.test(value))) throw new Error('Invalid CI release manifest.');
  for (const file of manifest.files) {
    if (typeof file.path !== 'string' || file.path.startsWith('/') || file.path.split('/').some((part) => ['', '.', '..'].includes(part)) || !/^[a-f0-9]{64}$/u.test(file.sha256)) throw new Error('Invalid release file record.');
    const path = join(directory, file.path); const info = await lstat(path);
    if (!info.isFile() || info.isSymbolicLink() || await realpath(path) !== path || hash(await readFile(path)) !== file.sha256) throw new Error('Release file checksum failed.');
  }
  for (const required of ['infra/compose.prod.yaml', 'infra/ops/deploy-production.mjs', 'infra/ops/deploy-production.sh', 'infra/ops/release.mjs', 'infra/ops/collect-metrics.mjs']) if (!manifest.files.some((file) => file.path === required)) throw new Error('Required deployment file missing from manifest.');
  return manifest;
}
export function validateCompose(model, manifest) {
  if (!model.services || Object.values(model.services).some((service) => service.build !== undefined)) throw new Error('Production Compose cannot contain build instructions.');
  const images = [];
  for (const [name, service] of Object.entries(model.services)) {
    if (!/^[a-zA-Z0-9._:/-]+@sha256:[a-f0-9]{64}$/u.test(service.image ?? '')) throw new Error('Every production service requires an immutable image digest.');
    if (['api', 'worker', 'demo-a-bff', 'demo-b-bff', 'migrate'].includes(name) && service.image !== manifest.images.runtime || name === 'edge' && service.image !== manifest.images.edge) throw new Error('Application image does not match the CI release manifest.');
    images.push(service.image);
  }
  for (const service of ['postgres', 'redis', 'api', 'worker', 'demo-a-bff', 'demo-b-bff', 'edge', 'migrate']) if (!model.services[service]) throw new Error('Required production service missing.');
  return [...new Set(images)];
}
async function controlledConfig(path) {
  const handle = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
  try { const info = await handle.stat(); if (!info.isFile() || info.nlink !== 1 || info.mode & 0o077 || info.size > 1024 * 1024 || process.getuid && info.uid !== process.getuid()) throw new Error('Use an owner-only production configuration file.'); return JSON.parse(await handle.readFile('utf8')); }
  finally { await handle.close(); }
}
export async function deploy({ directory, configPath, action, plan }, executor = executeCommand) {
  const manifest = await validateBundle(directory);
  const raw = await controlledConfig(configPath);
  const config = configuration({ ...raw, facts: { program: '/internal/identity-release-facts', args: [] }, mode: 'production', release: manifest.revision, composeFile: join(directory, 'infra/compose.prod.yaml'), rustImage: manifest.images.runtime, edgeImage: manifest.images.edge }, 'production');
  const env = { RUST_IMAGE: config.rustImage, EDGE_IMAGE: config.edgeImage };
  const compose = (args) => ['compose', '--project-name', config.project, '--project-directory', dirname(config.composeFile), '--env-file', config.envFile, '-f', config.composeFile, ...args];
  const result = await executor('docker', compose(['--profile', 'maintenance', 'config', '--format', 'json']), { env });
  if (result.code !== 0) throw new Error('Production Compose configuration failed; private values are not printed.');
  const images = validateCompose(JSON.parse(result.output), manifest);
  if (plan) return { plan: true, revision: manifest.revision, images, operations: ['pull immutable images', 'validate application configuration', 'start/check data dependencies', 'verify new encrypted backup', 'migrate (deploy only)', 'start with --no-build', 'wait for readiness', 'execute configured business smoke'] };
  if (!['deploy', 'rollback'].includes(action)) throw new Error('Use deploy or rollback.');
  await mkdir(config.stateDirectory, { recursive: true, mode: 0o700 }); const state = await stat(config.stateDirectory); if (state.mode & 0o077 || !state.isDirectory() || process.getuid && state.uid !== process.getuid()) throw new Error('Owner-controlled release state required.');
  const lock = join(config.stateDirectory, '.deployment-lock'); await mkdir(lock, { mode: 0o700 });
  try {
    let selected = images;
    if (action === 'rollback') { const previous = (await controlledConfig(join(config.stateDirectory, 'current.json'))).previous; if (!previous) throw new Error('No recorded previous release.'); selected = [...new Set([...images.filter((image) => !Object.values(manifest.images).includes(image)), previous.rustImage, previous.edgeImage])]; }
    for (const image of selected) {
      if (!/^[a-zA-Z0-9._:/-]+@sha256:[a-f0-9]{64}$/u.test(image)) throw new Error('Rollback image must be immutable.');
      if ((await executor('docker', ['pull', image])).code !== 0) throw new Error('Could not pull a required immutable image.');
    }
    if ((await executor('docker', compose(['up', '-d', '--no-build', '--wait', '--wait-timeout', '120', 'postgres', 'redis']), { env })).code !== 0) throw new Error('Data dependencies failed readiness.');
    // Guard every Compose up/run, including reviewed backup/facts/smoke commands.
    const runtimeExecutor = async (program, args, options) => {
      if (program === '/internal/identity-release-facts') {
        const facts = await databaseFacts({ ...config, rustImage: options.env.RUST_IMAGE, edgeImage: options.env.EDGE_IMAGE }, executor);
        return { code: 0, output: JSON.stringify(facts) };
      }
      return executor(program, program === 'docker' && args[0] === 'compose' && (args.includes('up') || args.includes('run')) && !args.includes('--no-build') ? [...args.slice(0, args.indexOf(args.includes('up') ? 'up' : 'run') + 1), '--no-build', ...args.slice(args.indexOf(args.includes('up') ? 'up' : 'run') + 1)] : args, options);
    };
    return await runRelease(config, action, { executor: runtimeExecutor });
  } finally { await rm(lock, { recursive: true }); }
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [action, configPath, confirmation] = process.argv.slice(2);
    if (!['deploy', 'rollback', 'plan'].includes(action) || !isAbsolute(configPath ?? '') || action !== 'plan' && confirmation !== '--allow-production' || process.argv.length !== (action === 'plan' ? 4 : 5)) throw new Error('Usage: deploy-production.sh plan /absolute/config.json OR deploy|rollback /absolute/config.json --allow-production');
    const result = await deploy({ directory: resolve(import.meta.dirname, '../..'), configPath, action, plan: action === 'plan' });
    console.log(result.plan ? JSON.stringify(result, null, 2) : 'Production release completed; stage evidence is in the controlled state directory.');
  } catch (error) { console.error(`Production release failed: ${error.message}`); process.exitCode = 1; }
}
