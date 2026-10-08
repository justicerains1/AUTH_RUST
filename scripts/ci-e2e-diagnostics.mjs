import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { sanitizeDiagnostic } from './ci-command.mjs';

const task = process.argv[2];
if (task !== 'T07') throw new Error('Only the explicit T07 diagnostic file is supported.');
try {
  const value = await readFile(resolve(import.meta.dirname, `../docs/evidence/${task}/e2e-diagnostics-sanitized.txt`), 'utf8');
  const safe = sanitizeDiagnostic(value);
  const annotation = safe.slice(-7000).replaceAll('%', '%25').replaceAll('\r', '%0D').replaceAll('\n', '%0A');
  console.error(`::error title=${task} browser diagnostics::${annotation}`);
} catch (error) {
  if (error.code !== 'ENOENT') throw error;
  console.log('No browser diagnostic report was generated; the original failing stage remains failed.');
}
