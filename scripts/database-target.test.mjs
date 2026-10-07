import assert from 'node:assert/strict';
import test from 'node:test';
import { migrationArguments, validateDatabaseTarget } from './database-target.mjs';

test('migrations require one explicit environment and production acknowledgement', () => {
  for (const args of [[], ['--env=test', '--env=test'], ['--env=production'], ['--env=test', '--allow-production'], ['--env=invalid']]) {
    assert.throws(() => migrationArguments(args));
  }
  assert.deepEqual(migrationArguments(['--env=production', '--allow-production']), { environment: 'production', allowProduction: true });
});

test('test database whitelist uses effective dbname and rejects production before connecting', () => {
  const url = 'postgres://identity:REDACTION_SENTINEL@127.0.0.1:1/identity_test';
  assert.equal(validateDatabaseTarget('test', url, { testOnly: true }).database, 'identity_test');
  for (const [environment, target] of [
    ['production', url], ['development', url], ['test', url.replace('/identity_test', '/identity_production')],
    ['test', `${url}?dbname=identity_production`], ['test', `${url}?options=-csearch_path%3Dpublic`],
  ]) assert.throws(() => validateDatabaseTarget(environment, target, { testOnly: true }), (error) => !error.message.includes('REDACTION_SENTINEL'));
});
