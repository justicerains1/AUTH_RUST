import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { runStage } from './full-test.mjs';
import { localEnvironment } from './database-target.mjs';
import { sanitizeDiagnostic } from './ci-command.mjs';

const root = resolve(import.meta.dirname, '..');
const result = process.env.CI_PREBUILT_DEV_IMAGES === '1'
  ? await runStage('docker', ['compose', '--env-file', resolve(root, '.local/dev.env'), '-f', resolve(root, 'infra/compose.dev.yaml'), 'up', '-d', '--wait', '--no-build'], { cwd: root })
  : await runStage(process.execPath, [resolve(root, 'scripts/run.mjs'), 'dev:up'], { cwd: root });
const directory = resolve(root, '.local/ci'); await mkdir(directory, { recursive: true, mode: 0o700 });
await writeFile(resolve(directory, 'dev-up.txt'), result.diagnostic, { mode: 0o600 });
const local = await localEnvironment(root);
const safe = sanitizeDiagnostic(result.diagnostic, { ...process.env, ...local });
process.stdout.write(safe);
if (result.exitCode !== 0) {
  const errors = safe.split(/\r?\n/u).filter((line) => /ERROR|failed|unhealthy|exited|cannot|denied/iu.test(line));
  const summary = [...errors.slice(-12), ...safe.trim().split(/\r?\n/u).slice(-12)].join('\n').slice(-6500);
  console.error(`::error title=Development environment startup failed::${summary.replaceAll('%', '%25').replaceAll('\r', '%0D').replaceAll('\n', '%0A')}`);
}
process.exitCode = result.exitCode;
