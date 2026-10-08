import { chmod, mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
import { localEnvironment, validateDatabaseTarget } from './database-target.mjs';
import { CommandError, runProcess } from './process.mjs';

// The CLI also rejects production and effective database overrides before touching files.
const root = resolve(import.meta.dirname, '..');
if (process.env.APP_ENV && !['development', 'test'].includes(process.env.APP_ENV)) throw new CommandError('Demo setup refuses production before reading local secrets.');
const local = await localEnvironment(root);
const url = new URL(local.DATABASE_URL); url.hostname = '127.0.0.1';
const target = validateDatabaseTarget('development', url.toString());
if (local.APP_ENV !== 'development' || target.database !== 'identity_development') throw new CommandError('Demo setup only accepts the local identity_development database.');
const directory = resolve(root, '.local/bff');
await mkdir(directory, { recursive: true, mode: 0o700 });
await chmod(directory, 0o700);
await runProcess('cargo', ['run', '--package', 'identity-store', '--bin', 'identity-demo-clients', '--locked'], { cwd: root, env: { ...process.env, APP_ENV: 'development', DATABASE_URL: url.toString(), BFF_DEMO_SECRETS_DIR: directory } });
