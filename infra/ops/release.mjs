// Explicit single-host release orchestration. No implicit migration, downgrade or volume deletion.
import { constants } from 'node:fs';
import { mkdir, open, readFile, rename, rm, stat } from 'node:fs/promises';
import { createHash, randomUUID } from 'node:crypto';
import { spawn } from 'node:child_process';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { backupCompletion } from './collect-metrics.mjs';

const digest = (text) => createHash('sha256').update(text).digest('hex');
function absolute(path) { if (typeof path !== 'string' || !isAbsolute(path) || resolve(path) !== path) throw new Error('Canonical absolute release paths required.'); return path; }
function image(value, local) { if (typeof value !== 'string' || (!/^[a-zA-Z0-9][a-zA-Z0-9._:/-]*@sha256:[a-f0-9]{64}$/u.test(value) && !(local && /^auth-rust-[a-z0-9-]+:[a-z0-9._-]+$/u.test(value)))) throw new Error('Reviewed immutable image required; local review tags need explicit acknowledgement.'); return value; }
function command(value) { if (!value || typeof value.program !== 'string' || !isAbsolute(value.program) || !Array.isArray(value.args) || value.args.some((argument) => typeof argument !== 'string')) throw new Error('A reviewed absolute command and literal argument array are required.'); return value; }
export function configuration(raw, acknowledgement) {
  const local = raw.mode === 'local-review';
  if (!local && raw.mode !== 'production' || local && acknowledgement !== 'local-review' || !local && acknowledgement !== 'production') throw new Error('Explicit acknowledged production or local review required.');
  if (!/^[a-z][a-z0-9-]{2,62}$/u.test(raw.project) || !/^[a-zA-Z0-9._-]{1,80}$/u.test(raw.release)) throw new Error('Explicit release/project identifiers required.');
  const services = raw.services ?? ['api', 'worker', 'demo-a-bff', 'demo-b-bff', 'edge'];
  if (!Array.isArray(services) || !services.length || new Set(services).size !== services.length || services.some((name) => !['api', 'worker', 'demo-a-bff', 'demo-b-bff', 'edge'].includes(name)) || !local && services.length !== 5) throw new Error('Release service selection must preserve the reviewed application set.');
  const checks = raw.configChecks ?? [['api', 'identity-server'], ['worker', 'identity-worker'], ['demo-a-bff', 'demo-bff'], ['demo-b-bff', 'demo-bff']];
  if (!Array.isArray(checks) || checks.some((entry) => !Array.isArray(entry) || entry.length !== 2 || !services.includes(entry[0]) || !['identity-server', 'identity-worker', 'demo-bff'].includes(entry[1]))) throw new Error('Explicit configuration checks required.');
  if (!local && JSON.stringify(checks) !== JSON.stringify([['api', 'identity-server'], ['worker', 'identity-worker'], ['demo-a-bff', 'demo-bff'], ['demo-b-bff', 'demo-bff']])) throw new Error('Production must validate every Rust application configuration.');
  return { ...raw, services, configChecks: checks, local, rustImage: image(raw.rustImage, local), edgeImage: image(raw.edgeImage, local),
    composeFile: absolute(raw.composeFile), envFile: absolute(raw.envFile), stateDirectory: absolute(raw.stateDirectory), backupDirectory: absolute(raw.backupDirectory),
    backup: command(raw.backup), smoke: command(raw.smoke), facts: command(raw.facts), compatibilityFile: raw.compatibilityFile ? absolute(raw.compatibilityFile) : undefined };
}
async function controlledJson(path, optional = false, serviceFile = false) {
  let handle;
  try {
    handle = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
    const info = await handle.stat();
    if (!info.isFile() || info.nlink !== 1 || info.size < 1 || info.size > 1024 * 1024 || (info.mode & 0o077) !== 0 || !serviceFile && process.getuid && info.uid !== process.getuid()) throw new Error('Owner-only release files required.');
    return JSON.parse(await handle.readFile('utf8'));
  } catch (error) { if (optional && error.code === 'ENOENT') return undefined; throw error; }
  finally { await handle?.close(); }
}
async function publish(path, value) {
  const temporary = `${path}.tmp-${randomUUID()}`; let handle;
  try { handle = await open(temporary, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL, 0o600); await handle.writeFile(JSON.stringify(value, null, 2) + '\n'); await handle.sync(); await handle.close(); handle = undefined; await rename(temporary, path); const parent = await open(dirname(path), constants.O_RDONLY); try { await parent.sync(); } finally { await parent.close(); } }
  finally { await handle?.close(); await rm(temporary, { force: true }); }
}
export async function executeCommand(program, args, options = {}) {
  return new Promise((done) => {
    const child = spawn(program, args, { cwd: options.cwd, env: { ...process.env, ...options.env }, shell: false, stdio: [options.input === undefined ? 'ignore' : 'pipe', 'pipe', 'pipe'] });
    if (options.input !== undefined) child.stdin.end(options.input);
    let output = ''; child.stdout.on('data', (part) => { if (output.length < 1024 * 1024) output += part; }); child.stderr.on('data', (part) => { if (output.length < 1024 * 1024) output += part; });
    child.once('error', () => done({ code: 127, output: 'Required release command unavailable.' })); child.once('close', (code) => done({ code: code ?? 1, output }));
  });
}
function compose(config, args) { return ['compose', '--project-name', config.project, '--project-directory', dirname(config.composeFile), '--env-file', config.envFile, '-f', config.composeFile, ...args]; }
// Only public key version IDs and checksummed migration metadata leave this collector.
// Explicit database selection must match the independently reviewed release configuration.
export async function databaseFacts(config, executor = executeCommand) {
  if (!/^[a-zA-Z_][a-zA-Z0-9_]{0,62}$/u.test(config.databaseUser) || !/^[a-zA-Z_][a-zA-Z0-9_]{0,62}$/u.test(config.databaseName) || !config.encryptionKeysFile) throw new Error('Explicit database and controlled encryption key-map path required for release facts.');
  const query = async (sql) => { const result = await executor('docker', compose(config, ['exec', '-T', 'postgres', 'psql', '-U', config.databaseUser, '-d', config.databaseName, '-Atc', sql]), { env: { RUST_IMAGE: config.rustImage, EDGE_IMAGE: config.edgeImage } }); if (result.code !== 0) throw new Error('Actual database compatibility query failed.'); return result.output.trim(); };
  let migrations = '[]'; let used = [];
  if (await query("SELECT CASE WHEN to_regclass('public._sqlx_migrations') IS NULL THEN 'absent' ELSE 'present' END") === 'present') {
    migrations = await query("SELECT COALESCE(json_agg(json_build_object('version',version,'checksum',encode(checksum,'hex'),'success',success) ORDER BY version),'[]'::json)::text FROM _sqlx_migrations");
    const rows = JSON.parse(migrations); if (!Array.isArray(rows) || rows.some((row) => row.success !== true)) throw new Error('Failed migration history cannot establish rollback compatibility.');
    used = (await query("SELECT DISTINCT kid FROM (SELECT encryption_kid AS kid FROM totp_factors UNION ALL SELECT encrypted_params->>'kid' FROM email_outbox WHERE encrypted_params IS NOT NULL UNION ALL SELECT state_encrypted->>'kid' FROM authentication_challenges WHERE state_encrypted IS NOT NULL UNION ALL SELECT encrypted_state->>'kid' FROM bff_login_flows UNION ALL SELECT encrypted_tokens->>'kid' FROM bff_sessions) versions WHERE kid IS NOT NULL ORDER BY kid")).split('\n').filter(Boolean);
  }
  const keys = await controlledJson(absolute(config.encryptionKeysFile), false, true);
  const facts = { version: 1, migration_sha256: digest(migrations), used_kids: used, configured_kids: Object.keys(keys).sort() };
  validateFacts(facts); return facts;
}
function validateFacts(facts) {
  if (facts.version !== 1 || !/^[a-f0-9]{64}$/u.test(facts.migration_sha256) || !Array.isArray(facts.used_kids) || !Array.isArray(facts.configured_kids) || [...facts.used_kids, ...facts.configured_kids].some((kid) => typeof kid !== 'string' || !/^[a-zA-Z0-9._-]{1,128}$/u.test(kid)) || facts.used_kids.some((kid) => !facts.configured_kids.includes(kid))) throw new Error('Actual schema/key compatibility facts are incomplete or incompatible.');
}
export async function runRelease(config, action, { executor = executeCommand, clock = () => Math.floor(Date.now() / 1000) } = {}) {
  if (!['deploy', 'rollback'].includes(action)) throw new Error('Use deploy or rollback.');
  await mkdir(config.stateDirectory, { recursive: true, mode: 0o700 });
  const directory = await stat(config.stateDirectory);
  if (!directory.isDirectory() || directory.mode & 0o077 || process.getuid && directory.uid !== process.getuid()) throw new Error('Owner-controlled release state directory required.');
  const lock = join(config.stateDirectory, '.release-lock'); await mkdir(lock, { mode: 0o700 });
  const record = { version: 1, action, release: config.release, project: config.project, mode: config.mode, started: clock(), stages: [], status: 'running' };
  const eventPath = join(config.stateDirectory, `${action}-${config.release}-${randomUUID()}.json`);
  const statePath = join(config.stateDirectory, 'current.json');
  let selected = { rustImage: config.rustImage, edgeImage: config.edgeImage };
  const run = async (stage, program, args, env = {}) => {
    const result = await executor(program, args, { env, cwd: dirname(config.composeFile) });
    // Raw output can contain connection details; it only enters an owner-controlled state directory.
    const rawPath = join(config.stateDirectory, `${randomUUID()}.diagnostic.txt`); const raw = await open(rawPath, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL, 0o600); try { await raw.writeFile(result.output ?? ''); } finally { await raw.close(); }
    record.stages.push({ stage, exitCode: result.code }); await publish(eventPath, record);
    if (result.code !== 0) throw new Error(`Release stage failed: ${stage}.`);
    return result.output;
  };
  try {
    const current = await controlledJson(statePath, true);
    let compatibility;
    if (action === 'rollback') {
      if (!current?.previous || !config.compatibilityFile) throw new Error('Rollback requires a recorded previous release and explicit schema/key compatibility evidence.');
      const evidence = await controlledJson(config.compatibilityFile);
      if (evidence.version !== 1 || evidence.from_release !== current.release || evidence.to_release !== current.previous.release || evidence.schema_compatible !== true || evidence.keys_compatible !== true || typeof evidence.reason !== 'string' || evidence.reason.length < 8 || evidence.old_rust_image !== current.previous.rustImage || evidence.old_edge_image !== current.previous.edgeImage || !Array.isArray(evidence.accepted_migration_sha256) || evidence.accepted_migration_sha256.some((hash) => !/^[a-f0-9]{64}$/u.test(hash)) || !Array.isArray(evidence.old_supported_kids) || evidence.old_supported_kids.some((kid) => typeof kid !== 'string' || !/^[a-zA-Z0-9._-]{1,128}$/u.test(kid))) throw new Error('Rollback compatibility evidence does not match the recorded release, schema range and key capabilities.');
      compatibility = evidence;
      selected = { rustImage: image(current.previous.rustImage, config.local), edgeImage: image(current.previous.edgeImage, config.local) };
      record.compatibilitySha256 = digest(await readFile(config.compatibilityFile));
    }
    const env = { RUST_IMAGE: selected.rustImage, EDGE_IMAGE: selected.edgeImage };
    record.images = selected;
    await run('compose-config', 'docker', compose(config, ['config', '--quiet']), env);
    for (const reference of Object.values(selected)) await run('image-present', 'docker', ['image', 'inspect', reference], env);
    for (const [service, binary] of config.configChecks) await run(`check-config-${service}`, 'docker', compose(config, ['run', '--rm', '--no-deps', service, binary, '--check-config']), env);
    const facts = JSON.parse(await run('current-schema-key-facts', config.facts.program, config.facts.args, env));
    validateFacts(facts);
    if (compatibility && (!compatibility.accepted_migration_sha256.includes(facts.migration_sha256) || facts.used_kids.some((kid) => !compatibility.old_supported_kids.includes(kid)) || current.previous.facts && current.previous.facts.used_kids.some((kid) => !compatibility.old_supported_kids.includes(kid)))) throw new Error('Actual current schema or encrypted records exceed reviewed previous-release compatibility.');
    record.facts = facts;
    const priorReceipt = await controlledJson(join(config.backupDirectory, 'last-success.json'), true);
    const startedBackup = clock();
    await run('base-backup', config.backup.program, config.backup.args, env);
    const completed = await backupCompletion(config.backupDirectory, clock());
    const receipt = await controlledJson(join(config.backupDirectory, 'last-success.json'));
    if (completed < startedBackup || receipt.backup_name === priorReceipt?.backup_name) throw new Error('A new verified backup must complete in this release attempt.');
    record.backup = { name: receipt.backup_name, sha256: receipt.ciphertext_sha256, completedAt: completed };
    record.stages.push({ stage: 'backup-verified', exitCode: 0 }); await publish(eventPath, record);
    let finalFacts = facts;
    if (action === 'deploy') {
      await run('migrate', 'docker', compose(config, ['--profile', 'maintenance', 'run', '--rm', '--no-deps', 'migrate']), env);
      finalFacts = JSON.parse(await run('post-migration-schema-key-facts', config.facts.program, config.facts.args, env));
      validateFacts(finalFacts);
    }
    await run('up', 'docker', compose(config, ['up', '-d', '--no-deps', ...config.services]), env);
    await run('ready', 'docker', compose(config, ['up', '-d', '--no-deps', '--wait', '--wait-timeout', '60', ...config.services]), env);
    await run('smoke', config.smoke.program, config.smoke.args, env);
    const release = action === 'rollback' ? current.previous.release : config.release;
    const next = { version: 1, release, project: config.project, ...selected, completed: clock(), backup: record.backup, facts: finalFacts, previous: action === 'deploy' && current ? { release: current.release, rustImage: current.rustImage, edgeImage: current.edgeImage, facts: current.facts } : undefined };
    await publish(statePath, next); record.status = 'complete'; record.completed = clock(); await publish(eventPath, record);
    return record;
  } catch (error) { record.status = 'failed'; record.failedAt = clock(); record.error = error.message; await publish(eventPath, record); throw error; }
  finally { await rm(lock, { recursive: true }); }
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    if (process.argv.length !== 5) throw new Error('Usage: release.mjs deploy|rollback|facts /absolute/owner-only-config.json --local-review|--allow-production.');
    const raw = await controlledJson(absolute(process.argv[3])); const config = configuration(raw, process.argv[4] === '--local-review' ? 'local-review' : process.argv[4] === '--allow-production' ? 'production' : undefined);
    if (process.argv[2] === 'facts') console.log(JSON.stringify(await databaseFacts(config)));
    else { await runRelease(config, process.argv[2]); console.log('Release operation completed; verified evidence is in the controlled state directory.'); }
  } catch { console.error('Release operation failed; review the owner-controlled configuration, stage record and private diagnostics.'); process.exitCode = 1; }
}
