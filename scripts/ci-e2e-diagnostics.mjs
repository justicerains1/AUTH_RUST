import { chmod, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { sanitizeDiagnostic } from './ci-command.mjs';

const FILES = { T07: 'T07-e2e-diagnostics.txt', T13: 'T13-integration-diagnostics.txt' };
const STAGES = new Set(['prepare', 'startup', 'browser', 'cleanup']);
export function diagnosticPath(root, task) {
  if (!Object.hasOwn(FILES, task)) throw new Error('Only explicit T07 or T13 diagnostics are supported.');
  return resolve(root, '.local/ci', FILES[task]);
}
export function sanitizeBrowserDiagnostic(value, environment = process.env) {
  let safe = sanitizeDiagnostic(value, environment);
  for (const [name, secret] of Object.entries(environment)) {
    if (/PASSWORD|SECRET|TOKEN|PRIVATE|CREDENTIAL|DATABASE_URL|REDIS_URL|SMTP|TEST_KEY/iu.test(name) && typeof secret === 'string' && secret.length >= 4) safe = safe.replaceAll(secret, '[REDACTED]');
  }
  return safe
    .replace(/\b(?:set-cookie|cookie)\s*:\s*[^\r\n]+/giu, 'Cookie: [REDACTED]')
    .replace(/([?&](?:code|state|nonce|token|access_token|refresh_token|id_token|id_token_hint|confirmation)=)[^&\s"'<>]+/giu, '$1[REDACTED]')
    .replace(/("(?:client_secret|access_token|refresh_token|id_token|csrf_token|password)"\s*:\s*")[^"\r\n]*(")/giu, '$1[REDACTED]$2')
    .replace(/\b[A-Za-z0-9_+/-]{43,}={0,2}/gu, '[REDACTED OPAQUE]')
    .replace(/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+/gu, '[REDACTED EMAIL]');
}
export async function clearCurrentDiagnostic(root, task) {
  await rm(diagnosticPath(root, task), { force: true });
}
export async function writeCurrentDiagnostic(root, task, stage, value, environment = process.env) {
  if (task !== 'T13' || !STAGES.has(stage)) throw new Error('Only known T13 integration stages can write diagnostics.');
  const path = diagnosticPath(root, task); const directory = resolve(root, '.local/ci');
  await mkdir(directory, { recursive: true, mode: 0o700 }); await chmod(directory, 0o700);
  const safe = sanitizeBrowserDiagnostic(value, environment).slice(-7000);
  await writeFile(path, `${task} current integration failure; stage=${stage}; generated=${new Date().toISOString()}\n${safe}\n`, { mode: 0o600 });
  await chmod(path, 0o600);
}
export async function publishDiagnostic(root, task, emit = console.error) {
  const path = diagnosticPath(root, task);
  let value;
  try { value = await readFile(path, 'utf8'); }
  catch (error) { if (error.code !== 'ENOENT') throw error; return false; }
  const annotation = sanitizeBrowserDiagnostic(value).slice(-7600).replaceAll('%', '%25').replaceAll('\r', '%0D').replaceAll('\n', '%0A');
  emit(`::error title=${task} ${task === 'T13' ? 'integration' : 'browser'} diagnostics::${annotation}`);
  return true;
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const published = await publishDiagnostic(resolve(import.meta.dirname, '..'), process.argv[2]);
    if (!published) console.log('No current diagnostic report was generated; the original failing stage remains failed.');
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
