import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { CommandError } from './process.mjs';

/** Parse the generated local file without printing any values or running shell expansion. */
export async function localEnvironment(root) {
  let content;
  try { content = await readFile(resolve(root, '.local', 'dev.env'), 'utf8'); }
  catch { throw new CommandError('缺少 .local/dev.env；先执行 npm run dev:secrets。'); }
  const environment = {};
  for (const line of content.split(/\r?\n/u)) {
    if (!line || line.startsWith('#')) continue;
    const at = line.indexOf('=');
    if (at > 0) environment[line.slice(0, at)] = line.slice(at + 1);
  }
  return environment;
}

export function validateDatabaseTarget(environment, databaseUrl, { allowProduction = false, testOnly = false } = {}) {
  if (!['development', 'test', 'production'].includes(environment)) throw new CommandError('必须显式选择 APP_ENV=development/test/production。');
  if (testOnly && environment !== 'test') throw new CommandError('测试拒绝 production/development；要求 APP_ENV=test 和 identity_test。');
  if (environment === 'production' && !allowProduction) throw new CommandError('生产迁移需要显式 --allow-production。');
  let url;
  try { url = new URL(databaseUrl); } catch { throw new CommandError('DATABASE_URL 无效或缺失。'); }
  const allowedQuery = ['sslmode', 'ssl-mode', 'sslrootcert', 'ssl-root-cert', 'ssl-ca', 'sslcert', 'ssl-cert', 'sslkey', 'ssl-key', 'statement-cache-capacity', 'host', 'hostaddr', 'port', 'dbname', 'user', 'password', 'application_name'];
  if (!['postgres:', 'postgresql:'].includes(url.protocol) || !url.hostname || !url.username || url.hash
    || [...url.searchParams.keys()].some((key) => !allowedQuery.includes(key))) throw new CommandError('DATABASE_URL 无效或含不支持的目标覆盖参数。');
  const database = url.searchParams.get('dbname') ?? decodeURIComponent(url.pathname.slice(1));
  if (!/^[a-zA-Z0-9_]{1,63}$/u.test(database)) throw new CommandError('DATABASE_URL 数据库名称无效。');
  if ((environment === 'test' || testOnly) && database !== 'identity_test') throw new CommandError('测试只允许 identity_test；执行前已拒绝非白名单目标。');
  return { url, database };
}

export function migrationArguments(args) {
  const selection = args.filter((arg) => /^--env=/u.test(arg));
  const allowProduction = args.includes('--allow-production');
  if (selection.length !== 1 || args.length !== selection.length + Number(allowProduction)) {
    throw new CommandError('用法：npm run db:migrate -- --env=development|test|production [--allow-production]。');
  }
  const environment = selection[0].slice('--env='.length);
  if (!['development', 'test', 'production'].includes(environment)) throw new CommandError('迁移环境必须是 development/test/production。');
  if (allowProduction && environment !== 'production') throw new CommandError('--allow-production 仅适用于显式 production 迁移。');
  if (environment === 'production' && !allowProduction) throw new CommandError('生产迁移需要显式 --allow-production。');
  return { environment, allowProduction };
}

export async function migrationEnvironment(root, selection) {
  let databaseUrl = process.env.DATABASE_URL;
  if (!databaseUrl && selection.environment === 'development') {
    const local = await localEnvironment(root);
    const { url } = validateDatabaseTarget('development', local.DATABASE_URL);
    url.hostname = '127.0.0.1';
    databaseUrl = url.toString();
  }
  if (!databaseUrl && selection.environment === 'test') databaseUrl = process.env.TEST_DATABASE_URL;
  validateDatabaseTarget(selection.environment, databaseUrl, selection);
  return { ...process.env, APP_ENV: selection.environment, DATABASE_URL: databaseUrl };
}
