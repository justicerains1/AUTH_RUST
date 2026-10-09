// Required short exact-SQL evidence; full performance runs are a separate explicit command.
import { resolve } from 'node:path';
import { runStage } from '../../scripts/full-test.mjs';
const root = resolve(import.meta.dirname, '../..');
const units = await runStage(process.execPath, ['--test', resolve(root, 'tests/load/capacity.test.mjs'), resolve(root, 'tests/load/scenarios.test.mjs')], { cwd: root });
const observer = await runStage('cargo', ['test', '-p', 'identity-server', '--test', 't21_load', '--locked', 'query_evidence::'], { cwd: root });
const evidence = await runStage(process.execPath, [resolve(root, 'tests/load/query-evidence.mjs')], { cwd: root });
const operational = await runStage(process.execPath, [resolve(root, 'tests/load/operational-query-evidence.mjs')], { cwd: root });
const observerPassed = observer.exitCode === 0 && /4 passed; 0 failed; 0 ignored/u.test(observer.diagnostic);
process.exitCode = units.exitCode !== 0 || !observerPassed || evidence.exitCode !== 0 || operational.exitCode !== 0 ? 1 : 0;
console.log(`T21 real exact-query probe exit ${evidence.exitCode}; operational admin/Worker probe exit ${operational.exitCode}; load measurement logic exit ${units.exitCode}; SQL observer/redaction ${observerPassed ? 'passed' : 'failed'}. Fifteen-minute/reference performance remains separate.`);
