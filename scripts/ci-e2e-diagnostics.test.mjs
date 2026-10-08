import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, rm, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import test from 'node:test';
import { clearCurrentDiagnostic, diagnosticPath, publishDiagnostic, sanitizeBrowserDiagnostic, writeCurrentDiagnostic } from './ci-e2e-diagnostics.mjs';

test('browser diagnostics preserve the actual failure while scrubbing headers, keys and response secrets', () => {
  const secret = 'known-config-secret'; const nonce = 'synthetic-nonce'; const opaque = 'p'.repeat(43);
  const value = `Test timeout of 60000ms exceeded\nCookie: identity-dev=short-cookie; csrf=value\nSet-Cookie: identity-dev=private\nBearer authorization-secret\n${secret}\nhttp://localhost:5192/callback?code=short-code&state=short-state&nonce=${nonce}\n{"access_token":"short-access","refresh_token":"short-refresh","id_token":"short-id-token","password":"short-password"}\n-----BEGIN PRIVATE KEY-----\nprivate-key-content\n-----END PRIVATE KEY-----\n${opaque}\nprivate@example.test`;
  const safe = sanitizeBrowserDiagnostic(value, { T13_TEST_KEY: secret });
  assert.match(safe, /Test timeout of 60000ms exceeded/u);
  for (const item of ['short-cookie','value\n','private\n','authorization-secret',secret,'short-code','short-state',nonce,'short-access','short-refresh','short-id-token','short-password','private-key-content',opaque,'private@example.test']) assert.ok(!safe.includes(item), item);
});

test('diagnostic paths reject unknown tasks and traversal before touching files', async () => {
  for (const task of ['T12','../T13','T13/../../secret','T07-e2e-diagnostics.txt']) {
    assert.throws(() => diagnosticPath('/tmp', task), /Only explicit/u);
    await assert.rejects(publishDiagnostic('/tmp', task), /Only explicit/u);
  }
  await assert.rejects(writeCurrentDiagnostic('/tmp', 'T13', 'arbitrary-stage', 'data'), /known T13/u);
});

test('current T13 diagnostics clear stale files and publish only newly generated known stages', async () => {
  const root = await mkdtemp(resolve(tmpdir(), 'identity-ci-diag-'));
  try {
    const path = diagnosticPath(root, 'T13'); await mkdir(resolve(root, '.local/ci'), { recursive: true }); await writeFile(path, 'STALE_FAILURE');
    await clearCurrentDiagnostic(root, 'T13'); const annotations = [];
    assert.equal(await publishDiagnostic(root, 'T13', (value) => annotations.push(value)), false); assert.equal(annotations.length, 0);
    await writeCurrentDiagnostic(root, 'T13', 'startup', 'Actual compiler failure\nCookie: secret', {});
    assert.match(await readFile(path, 'utf8'), /stage=startup/u);
    assert.equal(await publishDiagnostic(root, 'T13', (value) => annotations.push(value)), true);
    assert.match(annotations[0], /^::error title=T13 integration diagnostics::/u); assert.match(annotations[0], /Actual compiler failure/u); assert.ok(!annotations[0].includes('STALE_FAILURE')); assert.ok(!annotations[0].includes('\n'));
    if (process.platform !== 'win32') assert.equal((await stat(path)).mode & 0o777, 0o600);
    await writeCurrentDiagnostic(root, 'T13', 'browser', 'Expect status 200 received 503', {});
    assert.match(await readFile(path, 'utf8'), /stage=browser/u); assert.match(await readFile(path, 'utf8'), /received 503/u);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test('existing T07 diagnostic uses its own fixed file and annotation escaping', async () => {
  const root = await mkdtemp(resolve(tmpdir(), 'identity-ci-diag-'));
  try {
    await mkdir(resolve(root, '.local/ci'), { recursive: true }); await writeFile(diagnosticPath(root, 'T07'), 'current error 50%\n::warning::body');
    const annotations = []; assert.equal(await publishDiagnostic(root, 'T07', (value) => annotations.push(value)), true);
    assert.match(annotations[0], /^::error title=T07 browser diagnostics::/u); assert.match(annotations[0], /50%25%0A::warning::body/u);
  } finally { await rm(root, { recursive: true, force: true }); }
});
