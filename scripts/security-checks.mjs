import { access, mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { runStage } from './full-test.mjs';

const root = resolve(import.meta.dirname, '..');
export async function runSecurityChecks() {
  const gitleaks = process.env.GITLEAKS_BIN ?? resolve(root, '.local/security-tools/gitleaks/gitleaks');
  const deny = process.env.CARGO_DENY_BIN ?? resolve(root, '.local/security-tools/cargo-deny/cargo-deny-0.20.2-x86_64-unknown-linux-musl/cargo-deny');
  for (const file of [gitleaks, deny]) { try { await access(file); } catch { throw new Error('Security tools missing; install the documented pinned Gitleaks and cargo-deny.'); } }
  const stamp = new Date().toISOString().replace(/[:.]/gu, '-');
  const privateDirectory = resolve(root, `.local/security-checks/${stamp}`); await mkdir(privateDirectory, { recursive: true, mode: 0o700 });
  const npm = process.env.npm_execpath ?? '/usr/share/nodejs/npm/bin/npm-cli.js';
  const stages = [
    ['RustSec', 'cargo', ['audit', '--no-fetch', '--json']],
    ['Rust licenses and sources', deny, ['--locked', '--offline', '--format', 'json', 'check', 'licenses', 'sources']],
    ['npm vulnerability audit', process.execPath, [npm, 'audit', '--json']],
    ['Git history secret scan', gitleaks, ['git', '--redact=100', '--report-format', 'json', '--report-path', resolve(privateDirectory, 'gitleaks.json'), root]],
    ['Cross-module protocol/security', process.execPath, [resolve(root, 'tests/integration/T20.mjs')]],
    ['Production-build ZAP', process.execPath, [resolve(root, 'tests/security/zap.mjs')]],
  ];
  const results = [];
  for (const [name, program, args] of stages) {
    console.log(`Security check: ${name}`);
    const result = await runStage(program, args, { cwd: root });
    await writeFile(resolve(privateDirectory, `${results.length + 1}.txt`), result.diagnostic, { mode: 0o600 });
    results.push({ name, exitCode: result.exitCode }); console.log(`${name} exit ${result.exitCode}`);
  }
  const directory = resolve(root, 'docs/evidence/T20'); await mkdir(directory, { recursive: true });
  const exitCode = results.some((result) => result.exitCode !== 0) ? 1 : 0;
  await writeFile(resolve(directory, `security-checks-${stamp}.json`), JSON.stringify({ results, exitCode, scope: 'Dependency, secrets, cross-module protocol and bounded local ZAP; release/manual checks remain in acceptance.md' }, null, 2)+'\n');
  return exitCode;
}
