// Different groups run on independent GitHub runners; fault tests stay sequential within each.
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { runStage } from './full-test.mjs';
import { writeCurrentDiagnostic, sanitizeBrowserDiagnostic } from './ci-e2e-diagnostics.mjs';
import { localEnvironment } from './database-target.mjs';

const root = resolve(import.meta.dirname, '..');
const integration = (task) => ['test:integration', `--task=${task}`];
const e2e = (task) => ['test:e2e', `--task=${task}`];
export const groups = {
  authentication: [integration('T01'), integration('T03'), ['test:security', '--task=T04'], ...['T05', 'T06', 'T07', 'T08', 'T09'].flatMap((task) => [integration(task), e2e(task)])],
  protocol: [...['T10', 'T12'].flatMap((task) => [integration(task), ...(task === 'T10' ? [e2e(task), integration('T11')] : [e2e(task)])]), integration('T13'), integration('T14')],
  product: [e2e('T17'), e2e('T18'), e2e('T19'), ['test:accessibility']],
};
export const harnesses = {
  authentication: ['t04_security', 't05_accounts', 't06_sessions', 't07_passwords', 't08_mfa', 't09_passkeys'],
  protocol: ['t10_oauth', 't11_oidc', 't12_revocation', 't13_bff', 't14_admin'],
  product: ['t17_public', 't18_account', 't19_admin_ui', 't19_admin_management', 't23_accessibility'],
};
export async function prepareGroup(name, executor = runStage) {
  if (!Object.hasOwn(harnesses, name)) throw new Error('Unknown CI integration group.');
  if (name === 'authentication') {
    const store = await executor('cargo', ['test', '--package', 'identity-store', '--test', 't03_database', '--locked', '--no-run'], { cwd: root });
    process.stdout.write(sanitizeBrowserDiagnostic(store.diagnostic)); if (store.exitCode !== 0) return store.exitCode;
  }
  const result = await executor('cargo', ['test', '--package', 'identity-server', '--locked', '--no-run', ...harnesses[name].flatMap((target) => ['--test', target])], { cwd: root });
  process.stdout.write(sanitizeBrowserDiagnostic(result.diagnostic));
  return result.exitCode;
}
export async function runGroup(name, executor = runStage) {
  if (!Object.hasOwn(groups, name)) throw new Error('Unknown CI integration group.');
  for (const [command, task] of groups[name]) {
    console.log(`CI ${name}: ${command} ${task ?? ''}`);
    const result = await executor(process.execPath, [resolve(root, 'scripts/run.mjs'), command, ...(task ? [task] : [])], { cwd: root });
    if (result.exitCode !== 0) {
      let environment = process.env; try { environment = { ...process.env, ...await localEnvironment(root) }; } catch { /* missing config is itself a failure */ }
      if (task === '--task=T13') await writeCurrentDiagnostic(root, 'T13', 'browser', result.diagnostic, environment);
      console.error(sanitizeBrowserDiagnostic(result.diagnostic, environment).split('\n').slice(-24).join('\n'));
      return result.exitCode;
    }
  }
  return 0;
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try { process.exitCode = process.argv[3] === '--prepare' ? await prepareGroup(process.argv[2]) : await runGroup(process.argv[2]); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
