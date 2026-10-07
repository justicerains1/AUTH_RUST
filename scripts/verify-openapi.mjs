import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import SwaggerParser from '@apidevtools/swagger-parser';
import Ajv2020 from 'ajv/dist/2020.js';
import addFormats from 'ajv-formats';

const METHODS = ['get', 'post', 'put', 'patch', 'delete', 'head', 'options', 'trace'];
const LEVELS = new Set(['public', 'preauth', 'authenticated', 'recent-auth', 'strong-auth', 'admin', 'oidc-client']);
const SIGNED_IN_LEVELS = new Set(['authenticated', 'recent-auth', 'strong-auth', 'admin']);
const LIST_PATHS = ['/me/sessions', '/me/passkeys', '/me/grants', '/admin/users', '/admin/clients', '/admin/members', '/admin/audit-events'];

function requireCondition(condition, message) {
  if (!condition) throw new Error(message);
}

function nonempty(value) {
  return typeof value === 'string' && value.trim().length > 0;
}

/** Expand the actual route tables, including GET/POST and abbreviated sibling paths. */
export function expectedOperations(plan) {
  const protocol = plan.split('### 6.1 ')[1]?.split('### 6.2 ')[0];
  const api = plan.split('### 6.2 ')[1]?.split(/^## /mu)[0];
  requireCondition(protocol && api, 'plan.md 缺少第 6.1/6.2 节路由契约');
  const operations = [];
  for (const line of protocol.split('\n')) {
    const cells = line.split('|').map((cell) => cell.trim());
    if (!cells[1]?.startsWith('/')) continue;
    const methods = cells[2].match(/GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS/gu);
    requireCondition(methods?.length, `协议路由缺少方法：${cells[1]}`);
    for (const method of methods) operations.push(`${method} ${cells[1]}`);
  }
  for (const line of api.split('\n')) {
    if (!line.startsWith('| ')) continue;
    const cell = line.split('|')[2];
    let methods;
    let previous;
    for (const match of cell.matchAll(/(?:(GET(?:\/(?:GET|POST|PATCH|DELETE|PUT))*|POST|PATCH|DELETE|PUT)\s+)?(?<![\w])\/(auth|me|oauth|admin|confirm|verify|recovery)([a-z0-9/_{}-]*)/gu)) {
      if (match[1]) methods = match[1].split('/');
      let path = `/${match[2]}${match[3]}`;
      if (!/^\/(auth|me|oauth|admin)(\/|$)/u.test(path)) {
        requireCondition(previous && methods, `无法解析缩写路由：${path}`);
        path = path === '/recovery/verify' ? `/auth/mfa${path}` : `${previous.slice(0, previous.lastIndexOf('/'))}${path}`;
      }
      requireCondition(methods?.length, `JSON 路由缺少方法：${path}`);
      previous = path;
      for (const method of methods) operations.push(`${method} /api/v1${path}`);
    }
  }
  requireCondition(operations.length > 0, 'plan.md 没有可验证的路由');
  requireCondition(new Set(operations).size === operations.length, 'plan.md 路由方法出现重复');
  return operations;
}

function walk(value, callback, path = '#', seen = new Set()) {
  if (!value || typeof value !== 'object' || seen.has(value)) return;
  seen.add(value);
  callback(value, path);
  for (const [key, child] of Object.entries(value)) walk(child, callback, `${path}/${key}`, seen);
}

function schemaRoots(document) {
  const roots = Object.entries(document.components?.schemas ?? {}).map(([name, schema]) => [schema, `#/components/schemas/${name}`]);
  walk(document, (node, path) => {
    if (Object.hasOwn(node, 'schema')) roots.push([node.schema, `${path}/schema`]);
  });
  return roots;
}

function validateSchemas(document) {
  const ajv = new Ajv2020({ allErrors: true, strict: false, validateFormats: true });
  addFormats(ajv);
  const validators = new WeakMap();
  const visitedSchemaNodes = new Set();
  let schemas = 0;
  let examples = 0;
  function validator(schema, path) {
    requireCondition(schema && typeof schema === 'object' && Object.keys(schema).length > 0, `${path} 缺少具体 schema`);
    if (validators.has(schema)) return validators.get(schema);
    requireCondition(ajv.validateSchema(schema), `${path} JSON Schema 不合法：${ajv.errorsText()}`);
    let validate;
    try { validate = ajv.compile(schema); } catch (error) { throw new Error(`${path} schema 无法编译：${error.message}`, { cause: error }); }
    validators.set(schema, validate);
    schemas += 1;
    return validate;
  }
  function validateExample(schema, example, path) {
    const validate = validator(schema, path);
    requireCondition(validate(example), `${path} example 与 schema 不一致：${ajv.errorsText(validate.errors)}`);
    examples += 1;
  }
  for (const [schema, path] of schemaRoots(document)) {
    validator(schema, path);
    walk(schema, (node, nodePath) => {
      for (const example of node.examples ?? []) validateExample(node, example, nodePath);
      if (Object.hasOwn(node, 'example')) validateExample(node, node.example, nodePath);
    }, path, visitedSchemaNodes);
  }
  walk(document, (node, path) => {
    if (!node.schema) return;
    if (Object.hasOwn(node, 'example')) validateExample(node.schema, node.example, path);
    for (const example of Object.values(node.examples ?? {})) {
      if (Object.hasOwn(example, 'value')) validateExample(node.schema, example.value, path);
    }
  });
  return { schemas, examples };
}

function securitySchemes(operation, document, label) {
  requireCondition(Array.isArray(operation.security), `${label} 必须显式声明 security`);
  const names = operation.security.flatMap((requirement) => Object.keys(requirement));
  for (const name of names) requireCondition(document.components?.securitySchemes?.[name], `${label} 引用未定义安全方案 ${name}`);
  return names.map((name) => document.components.securitySchemes[name]);
}

function parametersFor(item, operation) {
  return [...(item.parameters ?? []), ...(operation.parameters ?? [])];
}

function hasHeader(parameters, name) {
  return parameters.some((parameter) => parameter.in === 'header' && parameter.name.toLowerCase() === name.toLowerCase() && parameter.required === true);
}

function checkMetadata(document, path, method, item, operation) {
  const label = `${method.toUpperCase()} ${path}`;
  const level = operation['x-auth-level'];
  requireCondition(LEVELS.has(level), `${label} x-auth-level 不合法或缺失`);
  requireCondition(nonempty(operation['x-auth-policy']), `${label} 缺少 x-auth-policy`);
  requireCondition(typeof operation['x-csrf-required'] === 'boolean', `${label} 缺少 x-csrf-required`);
  requireCondition(['planned', 'implemented'].includes(operation['x-implementation-status']), `${label} 缺少实现状态`);
  requireCondition(/^T\d{2}$/u.test(operation['x-task'] ?? ''), `${label} 缺少实现任务`);
  requireCondition(['none', 'single-use', 'idempotent'].includes(operation['x-idempotency']?.mode)
    && nonempty(operation['x-idempotency']?.description), `${label} x-idempotency 不完整`);
  requireCondition(nonempty(operation['x-rate-limit']?.policy)
    && ['deny', 'not-applicable'].includes(operation['x-rate-limit']?.onUnavailable), `${label} x-rate-limit 不完整`);
  const schemes = securitySchemes(operation, document, label);
  const isCookie = (scheme) => scheme.type === 'apiKey' && scheme.in === 'cookie';
  if (level === 'public') requireCondition(operation.security.length === 0, `${label} public 不应要求认证`);
  else {
    requireCondition(schemes.length > 0 && operation.security.every((entry) => Object.keys(entry).length > 0), `${label} 认证路由不能允许匿名 security`);
    if (level === 'oidc-client') requireCondition(schemes.every((scheme) => scheme.type === 'http' && scheme.scheme === 'basic'), `${label} 客户端必须仅使用 client_secret_basic`);
    else if (path === '/oauth/userinfo') requireCondition(schemes.every((scheme) => scheme.type === 'http' && scheme.scheme === 'bearer'), `${label} userinfo 必须使用 Bearer`);
    else requireCondition(schemes.every(isCookie), `${label} 浏览器身份路由必须使用 Cookie`);
  }
  if (path.startsWith('/api/v1/admin/')) requireCondition(level === 'admin', `${label} 管理端认证级别必须为 admin`);
  if (path === '/api/v1/me' || path.startsWith('/api/v1/me/')) requireCondition(SIGNED_IN_LEVELS.has(level), `${label} 账号路由必须要求已认证会话`);
  const parameters = parametersFor(item, operation);
  if (['get', 'head', 'options'].includes(method)) requireCondition(!operation['x-csrf-required'], `${label} 只读方法不能标记需要 CSRF`);
  if (path.startsWith('/api/v1/') && !['get', 'head', 'options'].includes(method)) requireCondition(operation['x-csrf-required'], `${label} 状态变更必须 CSRF`);
  if (operation['x-csrf-required']) {
    requireCondition(hasHeader(parameters, 'X-CSRF-Token') && hasHeader(parameters, 'Origin'), `${label} CSRF 必须声明 required Origin 与 X-CSRF-Token`);
  }
  for (const token of path.matchAll(/\{([^}]+)\}/gu)) {
    requireCondition(parameters.some((parameter) => parameter.in === 'path' && parameter.name === token[1]
      && parameter.required === true && parameter.schema?.format === 'uuid'), `${label} 路径 ${token[1]} 必须是 required UUID`);
  }
  if (path.startsWith('/api/v1/') || ['oidc-client', ...SIGNED_IN_LEVELS, 'preauth'].includes(level)) {
    requireCondition(operation['x-rate-limit'].onUnavailable === 'deny', `${label} 限流依赖失败必须拒绝`);
  }
}

function checkResponses(path, method, operation) {
  const label = `${method.toUpperCase()} ${path}`;
  const successes = Object.entries(operation.responses ?? {}).filter(([status]) => /^2\d{2}$/u.test(status));
  requireCondition(successes.length > 0, `${label} 缺少明确成功响应`);
  for (const [status, response] of successes) {
    if (status === '204') requireCondition(!response.content, `${label} 204 不允许响应体`);
    else requireCondition(Object.values(response.content ?? {}).some((media) => media.schema), `${label} ${status} 缺少成功 schema`);
  }
  for (const [status, response] of Object.entries(operation.responses)) {
    if (!/^[45]\d{2}$/u.test(status)) continue;
    const codes = operation['x-error-codes']?.[status];
    requireCondition(Array.isArray(codes) && codes.length > 0 && codes.every(nonempty), `${label} ${status} 缺少 x-error-codes`);
    const schema = response.content?.['application/json']?.schema;
    if (!schema && path === '/oauth/authorize') {
      requireCondition(response.content?.['text/html']?.schema?.type === 'string'
        && nonempty(response.description) && codes.every((code) => /^[a-z]+(?:_[a-z]+)*$/u.test(code)), `${label} ${status} 未验证回调错误页必须有 HTML schema 和标准 OAuth 错误码`);
      continue;
    }
    requireCondition(schema, `${label} ${status} 缺少错误 schema`);
    if (path.startsWith('/api/v1/')) {
      const details = schema.properties?.error ? [schema.properties.error] : (schema.oneOf ?? []).map((branch) => branch.properties?.error);
      requireCondition(details.length > 0 && details.every((detail) => detail?.properties?.code && detail.properties.message && detail.properties.request_id
        && ['code', 'message', 'request_id'].every((key) => detail.required?.includes(key))), `${label} ${status} 平台错误必须是 code/message/request_id 对象`);
      requireCondition(codes.every((code) => details.some((detail) => detail.properties.code.enum?.includes(code))), `${label} ${status} 错误码未列入 schema enum`);
    } else requireCondition(schema.properties?.error?.type === 'string', `${label} 协议错误必须使用标准字符串 error`);
  }
  if (path.startsWith('/api/v1/')) {
    requireCondition(operation.responses['429'] && operation.responses['503'], `${label} 必须定义 429 与 503`);
    requireCondition(operation.responses['429'].headers?.['Retry-After']?.schema, `${label} 429 必须定义 Retry-After`);
  }
}

function checkStandardFields(document) {
  const pkce = document.components?.schemas?.PkceChallenge;
  for (const example of pkce?.examples ?? []) {
    requireCondition(typeof example === 'string' && /^[A-Za-z0-9_-]{43}$/u.test(example)
      && Buffer.from(example, 'base64url').length === 32
      && Buffer.from(example, 'base64url').toString('base64url') === example, 'PKCE 示例必须是32字节规范base64url编码');
  }
  for (const path of LIST_PATHS.map((suffix) => `/api/v1${suffix}`)) {
    const item = document.paths[path];
    if (!item?.get) continue;
    const parameters = parametersFor(item, item.get);
    const limit = parameters.find((parameter) => parameter.in === 'query' && parameter.name === 'limit');
    const cursor = parameters.find((parameter) => parameter.in === 'query' && parameter.name === 'cursor');
    requireCondition(limit?.schema?.type === 'integer' && limit.schema.default === 20 && limit.schema.maximum === 100 && limit.schema.minimum >= 1, `${path} limit 必须默认20、最大100`);
    requireCondition(cursor?.schema?.type === 'string' && cursor.schema.maxLength && nonempty(cursor.description), `${path} 缺少有界不透明 cursor`);
    requireCondition(!parameters.some((parameter) => ['sort', 'order_by', 'sql'].includes(parameter.name)), `${path} 不接受任意 SQL 排序参数`);
    const schema = item.get.responses['200']?.content?.['application/json']?.schema;
    requireCondition(schema?.properties?.items?.type === 'array' && schema.properties.next_cursor
      && schema.required?.includes('items') && schema.required.includes('next_cursor'), `${path} 列表缺少 items/next_cursor`);
  }
  const scope = document.components?.schemas?.Scope;
  requireCondition(scope?.enum?.length === 3 && ['openid', 'profile', 'email'].every((value) => scope.enum.includes(value)), 'Scope 必须且仅支持 openid/profile/email');
  for (const path of ['/oauth/token', '/oauth/introspect', '/oauth/revoke']) {
    const operation = document.paths[path]?.post;
    if (!operation) continue;
    requireCondition(operation['x-auth-level'] === 'oidc-client'
      && operation.requestBody?.content?.['application/x-www-form-urlencoded']?.schema, `${path} 必须 Basic+form`);
    requireCondition(!parametersFor(document.paths[path], operation).some((parameter) => ['client_secret', 'password'].includes(parameter.name)), `${path} 不允许 URL 秘密参数`);
  }
}

export async function verifyOpenApiDocument(source, { expected, checkStandard = true } = {}) {
  const parser = new SwaggerParser();
  const raw = await parser.parse(source, { resolve: { external: false } });
  requireCondition(/^3\.1\./u.test(raw.openapi), '契约必须使用 OpenAPI 3.1');
  walk(raw, (node, path) => {
    if (node.$ref) requireCondition(node.$ref.startsWith('#/'), `${path} 仅允许文档内 $ref`);
  });
  const document = await parser.validate(raw, { resolve: { external: false }, dereference: { circular: false } });
  const schemaReport = validateSchemas(document);
  const actual = [];
  const operationIds = new Set();
  for (const [path, item] of Object.entries(document.paths)) {
    for (const method of METHODS) {
      const operation = item[method];
      if (!operation) continue;
      const label = `${method.toUpperCase()} ${path}`;
      actual.push(label);
      requireCondition(nonempty(operation.operationId) && !operationIds.has(operation.operationId), `${label} operationId 缺失或重复`);
      operationIds.add(operation.operationId);
      checkMetadata(document, path, method, item, operation);
      checkResponses(path, method, operation);
    }
  }
  requireCondition(actual.length > 0, '契约没有真实 operation');
  if (expected) {
    const missing = expected.filter((operation) => !actual.includes(operation));
    const unexpected = actual.filter((operation) => !expected.includes(operation));
    requireCondition(!missing.length && !unexpected.length, `plan.md 路由覆盖不一致；缺少：[${missing.join(', ')}]；额外：[${unexpected.join(', ')}]`);
  }
  if (checkStandard) checkStandardFields(document);
  return { ...schemaReport, operations: actual.length, jsonOperations: actual.filter((operation) => operation.includes(' /api/v1/')).length };
}

export async function verifyOpenApiWorkspace(root = resolve(dirname(fileURLToPath(import.meta.url)), '..')) {
  const plan = await readFile(resolve(root, 'plan.md'), 'utf8');
  const source = resolve(root, 'docs', 'api', 'openapi.yaml');
  try { await readFile(source); } catch { throw new Error('缺少 docs/api/openapi.yaml；按 T02 编写真实契约后重试。'); }
  return verifyOpenApiDocument(source, { expected: expectedOperations(plan) });
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const report = await verifyOpenApiWorkspace();
    console.log(`OpenAPI 检查通过：${report.operations} 个操作（${report.jsonOperations} 个 JSON API）、${report.schemas} 个 schema、${report.examples} 个 examples；契约验证不代表功能已实现。`);
  } catch (error) {
    console.error(`OpenAPI 检查失败：${error.message}`);
    process.exitCode = 1;
  }
}
