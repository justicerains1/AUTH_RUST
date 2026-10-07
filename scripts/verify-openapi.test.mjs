import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import test from 'node:test';
import { expectedOperations, verifyOpenApiDocument, verifyOpenApiWorkspace } from './verify-openapi.mjs';

// Deliberately small fixture: validates tooling behavior only, not authentication.
function fixture() {
  return {
    openapi: '3.1.0', info: { title: 'Validator unit fixture', version: '1.0.0' },
    components: { schemas: { Value: { type: 'object', additionalProperties: false, properties: { value: { type: 'string' } }, required: ['value'] } } },
    paths: {
      '/fixture': {
        get: {
          operationId: 'fixtureRead', security: [],
          'x-auth-level': 'public', 'x-auth-policy': 'Fixture is public.', 'x-csrf-required': false,
          'x-implementation-status': 'planned', 'x-task': 'T02',
          'x-idempotency': { mode: 'none', description: 'Read-only fixture.' },
          'x-rate-limit': { policy: 'fixture', onUnavailable: 'not-applicable' }, 'x-error-codes': {},
          responses: { '200': { description: 'Typed fixture', content: { 'application/json': { schema: { $ref: '#/components/schemas/Value' }, example: { value: 'sample' } } } } },
        },
      },
    },
  };
}

function verifyFixture(document) {
  return verifyOpenApiDocument(document, { expected: ['GET /fixture'], checkStandard: false });
}

test('valid 3.1 local-ref fixture is actually parsed and its examples validated', async () => {
  const report = await verifyFixture(fixture());
  assert.equal(report.operations, 1);
  assert.ok(report.schemas > 0);
  assert.equal(report.examples, 1);
});

const mutations = [
  ['missing document metadata', (doc) => { delete doc.info; }, /schema validation failed/u],
  ['unresolved reference', (doc) => { doc.paths['/fixture'].get.responses['200'].content['application/json'].schema.$ref = '#/components/schemas/Missing'; }, /does not exist|not found/u],
  ['external reference', (doc) => { doc.paths['/fixture'].get.responses['200'].content['application/json'].schema.$ref = 'https://example.invalid/schema'; }, /仅允许文档内/u],
  ['invalid JSON Schema type', (doc) => { doc.components.schemas.Value.properties.value.type = 'invented-type'; }, /JSON Schema 不合法/u],
  ['example inconsistent with schema', (doc) => { doc.paths['/fixture'].get.responses['200'].content['application/json'].example.value = 7; }, /example 与 schema 不一致/u],
  ['missing success schema', (doc) => { delete doc.paths['/fixture'].get.responses['200'].content['application/json'].schema; }, /缺少成功 schema/u],
  ['unknown auth level', (doc) => { doc.paths['/fixture'].get['x-auth-level'] = 'anything'; }, /x-auth-level/u],
  ['missing explicit security', (doc) => { delete doc.paths['/fixture'].get.security; }, /显式声明 security/u],
  ['anonymous protected security', (doc) => { doc.paths['/fixture'].get['x-auth-level'] = 'authenticated'; doc.paths['/fixture'].get.security = [{}]; }, /不能允许匿名/u],
  ['missing CSRF metadata', (doc) => { delete doc.paths['/fixture'].get['x-csrf-required']; }, /x-csrf-required/u],
  ['CSRF on read-only operation', (doc) => { doc.paths['/fixture'].get['x-csrf-required'] = true; }, /只读方法/u],
  ['missing idempotency description', (doc) => { delete doc.paths['/fixture'].get['x-idempotency'].description; }, /x-idempotency/u],
  ['invalid rate limit failure policy', (doc) => { doc.paths['/fixture'].get['x-rate-limit'].onUnavailable = 'allow'; }, /x-rate-limit/u],
  ['missing route', (doc) => { delete doc.paths['/fixture'].get; }, /没有真实 operation/u],
];

for (const [name, mutate, expected] of mutations) {
  test(`fixture rejects ${name}`, async () => {
    const document = fixture(); mutate(document);
    await assert.rejects(verifyFixture(document), expected);
  });
}

test('extra paths cannot silently bypass plan coverage', async () => {
  const document = fixture();
  document.paths['/extra'] = structuredClone(document.paths['/fixture']);
  document.paths['/extra'].get.operationId = 'extraRead';
  await assert.rejects(verifyFixture(document), /路由覆盖不一致/u);
});

test('actual plan 6.1/6.2 declarations expand methods and sibling paths', async () => {
  const plan = await readFile(resolve(import.meta.dirname, '..', 'plan.md'), 'utf8');
  const expected = expectedOperations(plan);
  assert.ok(expected.includes('GET /oauth/authorize'));
  assert.ok(expected.includes('POST /oauth/authorize'));
  assert.ok(expected.includes('GET /oauth/logout'));
  assert.ok(expected.includes('POST /oauth/logout'));
  assert.ok(expected.includes('POST /api/v1/auth/mfa/recovery/verify'));
  assert.ok(expected.includes('POST /api/v1/me/passkeys/registration/verify'));
  assert.equal(expected.filter((operation) => operation.includes(' /api/v1/')).length, 48);
});

test('actual OpenAPI validates every plan operation and real schema/example', async () => {
  const report = await verifyOpenApiWorkspace();
  assert.equal(report.operations, 59);
  assert.equal(report.jsonOperations, 48);
  assert.ok(report.schemas > 100);
  assert.ok(report.examples > 100);
});

async function actualDocument() {
  return JSON.parse(await readFile(resolve(import.meta.dirname, '..', 'docs/api/openapi.yaml'), 'utf8'));
}

const contractMutations = [
  ['non-canonical PKCE example', (doc) => { doc.components.schemas.PkceChallenge.examples = ['C'.repeat(43)]; }, /PKCE 示例/u],
  ['administrator auth downgrade', (doc) => { doc.paths['/api/v1/admin/users'].get['x-auth-level'] = 'authenticated'; }, /必须为 admin/u],
  ['missing mutating CSRF', (doc) => { doc.paths['/api/v1/auth/register'].post['x-csrf-required'] = false; }, /状态变更必须 CSRF/u],
  ['missing Origin requirement', (doc) => { doc.components.parameters.Origin.required = false; }, /required Origin/u],
  ['non-UUID path id', (doc) => { doc.components.schemas.Uuid.format = 'email'; }, /example 与 schema|UUID/u],
  ['unknown API error code', (doc) => { doc.paths['/api/v1/me'].get['x-error-codes']['401'] = ['UNKNOWN_UNDECLARED']; }, /错误码未列入/u],
  ['list limit beyond maximum', (doc) => { doc.components.parameters.Limit.schema.maximum = 1000; }, /limit 必须/u],
  ['scope expansion', (doc) => { doc.components.schemas.Scope.enum.push('admin'); }, /仅支持/u],
  ['token without form', (doc) => { delete doc.paths['/oauth/token'].post.requestBody; }, /Basic\+form/u],
  ['authorize HTML error without schema', (doc) => { delete doc.paths['/oauth/authorize'].get.responses['400'].content['text/html'].schema; }, /HTML schema/u],
  ['authorize nonstandard error code', (doc) => { doc.paths['/oauth/authorize'].get['x-error-codes']['400'] = ['API_CHINESE_ENVELOPE']; }, /标准 OAuth/u],
];

for (const [name, mutate, expected] of contractMutations) {
  test(`actual contract rejects ${name}`, async () => {
    const document = await actualDocument(); mutate(document);
    await assert.rejects(verifyOpenApiDocument(document), expected);
  });
}
