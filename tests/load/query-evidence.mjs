// Explicit short SQL/endpoint probe. This never runs k6 or a performance measurement.
import { spawn } from 'node:child_process';
import { resolve } from 'node:path';

const child = spawn(process.execPath, [resolve(import.meta.dirname, 'run.mjs'), '--probe-only'], { shell: false, stdio: 'inherit' });
child.on('error', () => { console.error('SQL evidence probe could not start.'); process.exitCode = 1; });
child.on('close', (code) => { process.exitCode = code ?? 1; });
