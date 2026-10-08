import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import test from 'node:test';
import { runSuiteManifest, suites } from './suite-manifest.mjs';

test('complete suites fail for missing task files before running any child', async () => {
  const root = await mkdtemp(resolve(tmpdir(), 'identity-suites-'));
  try { await assert.rejects(runSuiteManifest('e2e', { root, executor: () => { throw new Error('must not execute'); } }), /Required e2e suite T05 is missing/u); }
  finally { await rm(root, { recursive: true, force: true }); }
});
test('complete suites keep later tasks after a failed module and preserve nonzero status', async () => {
  const root = await mkdtemp(resolve(tmpdir(), 'identity-suites-'));
  try {
    await mkdir(resolve(root, 'tests/integration'), { recursive: true });
    for (const task of suites.e2e) await writeFile(resolve(root, `tests/integration/${task}.mjs`), '');
    const visited = [];
    const code = await runSuiteManifest('e2e', { root, executor: async (_program, args) => { const task = args[0].split(/[\\/]/u).at(-1).slice(0, 3); visited.push(task); return { exitCode: task === 'T07' ? 7 : 0, diagnostic: 'PRIVATE_FIXTURE_DIAGNOSTIC' }; } });
    assert.equal(code, 1); assert.deepEqual(visited, suites.e2e);
  } finally { await rm(root, { recursive: true, force: true }); }
});
