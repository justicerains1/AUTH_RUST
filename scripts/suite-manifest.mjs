import { access, mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { runStage } from './full-test.mjs';

export const suites = {
  integration: ['T01', 'T03', 'T04', 'T05', 'T06', 'T07', 'T08', 'T09', 'T10', 'T11', 'T12', 'T13', 'T14', 'T17', 'T18', 'T19', 'T20'],
  e2e: ['T05', 'T06', 'T07', 'T08', 'T09', 'T10', 'T12', 'T13', 'T17', 'T18', 'T19'],
};

/** Every required module runs; missing files fail before execution, failures never suppress later suites. */
export async function runSuiteManifest(kind, { root = resolve(import.meta.dirname, '..'), executor = runStage } = {}) {
  if (!Object.hasOwn(suites, kind)) throw new Error('Unsupported complete suite.');
  const stages = [];
  for (const task of suites[kind]) {
    const script = resolve(root, `tests/integration/${task}.mjs`);
    try { await access(script); } catch { throw new Error(`Required ${kind} suite ${task} is missing.`); }
    stages.push({ task, script });
  }
  const stamp = new Date().toISOString().replace(/[:.]/gu, '-');
  const privateDirectory = resolve(root, `.local/complete-${kind}/${stamp}`);
  await mkdir(privateDirectory, { recursive: true, mode: 0o700 });
  const results = [];
  for (const { task, script } of stages) {
    console.log(`Complete ${kind}: ${task}`);
    const start = Date.now();
    const result = await executor(process.execPath, [script, ...(kind === 'e2e' ? ['--e2e'] : [])], { cwd: root });
    await writeFile(resolve(privateDirectory, `${task}.txt`), result.diagnostic, { mode: 0o600 });
    results.push({ task, exitCode: result.exitCode, durationMs: Date.now() - start });
    console.log(`${task} exit ${result.exitCode}`);
  }
  const evidence = resolve(root, `docs/evidence/complete-${kind}`);
  await mkdir(evidence, { recursive: true });
  await writeFile(resolve(evidence, `${stamp}.json`), JSON.stringify({ kind, completed: new Date().toISOString(), results, exitCode: results.some((result) => result.exitCode !== 0) ? 1 : 0 }, null, 2)+'\n');
  return results.some((result) => result.exitCode !== 0) ? 1 : 0;
}
