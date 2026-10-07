import { mkdir, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { runStage } from './full-test.mjs';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const COMMANDS = new Set(['check', 'test:unit', 'build']);

/** Public CI annotations contain diagnostics, never configuration or long secret material. */
export function sanitizeDiagnostic(value, environment = process.env) {
  const ansi = new RegExp(`${String.fromCharCode(27)}\\[[0-?]*[ -/]*[@-~]`, 'gu');
  let text = value.replace(ansi, '');
  for (const [name, secret] of Object.entries(environment)) {
    if (/PASSWORD|SECRET|TOKEN|PRIVATE|CREDENTIAL|DATABASE_URL|REDIS_URL|SMTP/iu.test(name) && secret?.length >= 8) {
      text = text.replaceAll(secret, '[REDACTED]');
    }
  }
  return text
    .replace(/-----BEGIN [^-]*PRIVATE KEY-----[\s\S]*?(?:-----END [^-]*PRIVATE KEY-----|$)/gu, '[REDACTED PRIVATE KEY]')
    .replace(/\b(?:postgres(?:ql)?|rediss?):\/\/[^\s"'<>]+/giu, '[REDACTED CONNECTION]')
    .replace(/\b(?:Basic|Bearer)\s+[A-Za-z0-9+/_.=-]+/gu, '[REDACTED AUTHORIZATION]')
    .replace(/\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+/gu, '[REDACTED TOKEN]')
    .replace(/[A-Za-z0-9+/_=-]{64,}/gu, '[REDACTED LONG VALUE]');
}

function annotation(value) {
  return value.replaceAll('%', '%25').replaceAll('\r', '%0D').replaceAll('\n', '%0A');
}

export async function runCiCommand(command) {
  if (!COMMANDS.has(command)) throw new Error('CI command must be check, test:unit or build.');
  const result = await runStage(process.execPath, [resolve(ROOT, 'scripts/run.mjs'), command]);
  const directory = resolve(ROOT, '.local/ci');
  await mkdir(directory, { recursive: true, mode: 0o700 });
  await writeFile(resolve(directory, `${command.replaceAll(':', '-')}.txt`), result.diagnostic, { mode: 0o600 });
  const safe = sanitizeDiagnostic(result.diagnostic);
  process.stdout.write(safe);
  if (result.exitCode !== 0) {
    const lines = safe.trim().split(/\r?\n/gu);
    const diagnostic = lines.slice(-24).join('\n').slice(-5000);
    console.error(`::error title=CI ${command} failed::${annotation(diagnostic || 'No child diagnostics available.')}`);
  }
  return result.exitCode;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try { process.exitCode = await runCiCommand(process.argv[2]); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
