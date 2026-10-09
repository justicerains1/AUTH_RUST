// Short real administrator/Worker probe. Never starts/stops dependencies or runs a load generator.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash, randomBytes } from 'node:crypto';
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { localEnvironment, validateDatabaseTarget } from '../../scripts/database-target.mjs';
import { sanitizeBrowserDiagnostic } from '../../scripts/ci-e2e-diagnostics.mjs';

const root = resolve(import.meta.dirname, '../..');
const privateDirectory = resolve(root, '.local', 't21-operational-'+randomBytes(8).toString('hex'));
const evidence = resolve(root, 'docs/evidence/T21');
const stamp = new Date().toISOString().replaceAll(':', '-').replaceAll('.', '-');
const report = { started: new Date().toISOString(), scope: 'Real local admin/TOTP/100k pagination and SMTP Worker query probe, not capacity or production acceptance', failures: [] };
let environment = process.env;
async function main() {
  const supplied = process.env.TEST_DATABASE_URL ?? process.env.DATABASE_URL;
  if (supplied) validateDatabaseTarget(process.env.APP_ENV ?? 'test', supplied, { testOnly: true });
  else assert.equal(process.env.APP_ENV ?? 'test', 'test');
  const local = await localEnvironment(root); const url = new URL(supplied ?? local.DATABASE_URL);
  if (!supplied) { url.hostname = '127.0.0.1'; url.pathname = '/identity_test'; url.search = ''; }
  validateDatabaseTarget('test', url.toString(), { testOnly: true });
  const redis = new URL(local.REDIS_URL); redis.hostname = '127.0.0.1';
  await mkdir(privateDirectory, { recursive: true, mode: 0o700 }); const key = randomBytes(32).toString('base64'); const keyFile = resolve(privateDirectory, 'keys.json');
  await writeFile(keyFile, JSON.stringify({ operational: key }), { mode: 0o600 });
  environment = { ...process.env, APP_ENV: 'test', DATABASE_URL: url.toString(), TEST_DATABASE_URL: url.toString(), REDIS_URL: redis.toString(), T21_OPERATIONAL_PROBE: '1', T21_OP_PRIVATE_DIRECTORY: privateDirectory, T21_OP_SIGNING_KEY_FILE: resolve(root, '.local/signing.pem'), T21_OP_ENCRYPTION_KEYS_FILE: keyFile, T21_OP_ACTIVE_ENCRYPTION_KID: 'operational', T21_OP_PASSWORD: 'Operational query '+randomBytes(24).toString('hex'), T21_OP_NODE: process.execPath };
  const paths = ['crates/identity-server/tests/t21_operational_queries.rs', 'tests/load/query-evidence.rs', 'tests/load/operational-query-evidence.mjs', 'crates/identity-store/src/admin.rs', 'crates/identity-store/src/repository.rs', 'crates/identity-worker/src/outbox.rs', 'Cargo.lock'];
  report.sourceSha256 = {};
  for (const path of paths) report.sourceSha256[path] = createHash('sha256').update(await readFile(resolve(root, path))).digest('hex');
  const child = spawn(process.env.T21_CARGO ?? '/root/.cargo/bin/cargo', ['test', '-p', 'identity-server', '--test', 't21_operational_queries', '--locked', '--', '--exact', 't21_operational_queries', '--nocapture'], { cwd: root, env: environment, shell: false, stdio: ['ignore', 'pipe', 'pipe'] });
  let raw = ''; child.stdout.setEncoding('utf8'); child.stderr.setEncoding('utf8'); child.stdout.on('data', (part) => { raw += part; }); child.stderr.on('data', (part) => { raw += part; });
  const code = await new Promise((done, reject) => { child.on('error', () => reject(new Error('Operational SQL probe tool unavailable.'))); child.on('close', (code) => done(code ?? 1)); });
  report.testExitCode = code;
  const safe = sanitizeBrowserDiagnostic(raw, { ...environment, T21_OP_KEY_SECRET: key }).replace(/\b[0-9a-f]{8}-[0-9a-f-]{27}\b/giu, '[UUID]');
  await mkdir(evidence, { recursive: true });
  if (code !== 0) await writeFile(resolve(evidence, 'sql-operational-'+stamp+'-failure.txt'), safe, { flag: 'wx' });
  try { report.evidence = JSON.parse(await readFile(resolve(privateDirectory, 'operational-query-evidence.json'), 'utf8')); }
  catch { report.failures.push('No complete real operational query evidence was produced.'); }
  report.sourceStableDuringRun = true;
  for (const path of paths) if (report.sourceSha256[path] !== createHash('sha256').update(await readFile(resolve(root, path))).digest('hex')) report.sourceStableDuringRun = false;
  if (!report.sourceStableDuringRun) report.failures.push('Source changed during the operational probe; binary/source equivalence cannot be assumed.');
  if (code !== 0) report.failures.push('Real operational SQL probe failed; preserved sanitized details.');
}
try { await main(); } catch (error) { report.failures.push(error.message); }
finally {
  report.completed = new Date().toISOString(); report.exitCode = report.failures.length ? 1 : 0;
  await mkdir(evidence, { recursive: true });
  await writeFile(resolve(evidence, 'sql-operational-'+stamp+(report.exitCode ? '-failure' : '')+'.json'), JSON.stringify(report, null, 2)+'\n', { flag: 'wx' });
  await rm(privateDirectory, { recursive: true, force: true });
  console.log('T21 operational SQL probe exit '+report.exitCode+'; sanitized report preserved.');
  process.exitCode = report.exitCode;
}
