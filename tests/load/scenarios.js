/* global __ENV, open */
import http from 'k6/http';
import { Counter, Rate, Trend } from 'k6/metrics';
import execution from 'k6/execution';

const credentials = JSON.parse(open(__ENV.T21_CREDENTIALS));
if (__ENV.APP_ENV !== 'test' || !/^http:\/\/127\.0\.0\.1:531[01]$/u.test(credentials.base) || credentials.control !== 'http://127.0.0.1:5311' || credentials.clients.length !== 20 || credentials.sessions.length !== 2048) throw new Error('Explicit isolated test-only load fixture required.');
const selected = __ENV.T21_SCENARIO;
if (!['introspection', 'account', 'password', 'mixed'].includes(selected)) throw new Error('Unknown load scenario.');
const introspectionDuration = new Trend('introspection_duration', true);
const accountDuration = new Trend('account_duration', true);
const passwordDuration = new Trend('password_duration', true);
const passwordEndpointDuration = new Trend('password_endpoint_duration', true);
const unexpectedErrors = new Rate('unexpected_errors');
const rejected429 = new Counter('rate_limited_requests');
const invalidAuthority = new Counter('invalid_authority');
const completed = new Counter('measurement_requests');
const serviceFailures = new Counter('server_5xx');
const rates = selected === 'mixed' ? { introspection: 300, account: 50, password: 5 } : { [selected]: { introspection: 300, account: 100, password: 5 }[selected] };
const scenarios = {};
for (const [name, rate] of Object.entries(rates)) scenarios[name] = { executor: 'constant-arrival-rate', exec: name, rate, timeUnit: '1s', duration: '17m', preAllocatedVUs: name === 'password' ? 16 : 128, maxVUs: name === 'password' ? 32 : 512, gracefulStop: '10s', tags: { endpoint: name } };
const thresholds = { unexpected_errors: ['rate<0.001'], invalid_authority: ['count==0'], rate_limited_requests: ['count==0'], dropped_iterations: ['count==0'] };
for (const [name, rate] of Object.entries(rates)) thresholds[`measurement_requests{endpoint:${name}}`] = [`count>=${String(rate * 900)}`];
if (rates.introspection) thresholds.introspection_duration = ['p(95)<=100', 'p(99)<=250'];
if (rates.account) thresholds.account_duration = ['p(95)<=200'];
if (rates.password) thresholds.password_duration = ['p(95)<=1000'];
export const options = { scenarios, thresholds, discardResponseBodies: true, summaryTrendStats: ['avg', 'min', 'med', 'max', 'p(75)', 'p(95)', 'p(99)'], systemTags: ['status', 'method', 'name', 'scenario', 'error_code'], noConnectionReuse: false, userAgent: 'Identity T21 load fixture', insecureSkipTLSVerify: false };
let tokenPool;
let tokenUpdated = 0;
function measured() { const elapsed = Date.now() - execution.scenario.startTime; return elapsed >= 120_000 && elapsed < 1_020_000; }
function pool(data) { if (tokenPool === undefined) { tokenPool = data.tokens; tokenUpdated = Date.now(); } if (tokenPool === undefined || Date.now() - tokenUpdated >= 60_000) { const response = http.get(`${credentials.control}/pool`, { headers: { 'x-test-key': credentials.key }, responseType: 'text', tags: { name: 'test-control-pool', endpoint: 'control' } }); if (response.status !== 200) throw new Error('Controlled current token pool unavailable.'); tokenPool = response.json('tokens'); tokenUpdated = Date.now(); } return tokenPool; }
function source(index) { return `198.19.${Math.floor(index / 250) % 250}.${index % 250 + 1}`; }
function record(name, response, duration, valid, measurementPhase) {
  if (!measurementPhase) return;
  const metricTrend = name === 'introspection' ? introspectionDuration : name === 'account' ? accountDuration : passwordDuration;
  metricTrend.add(duration); completed.add(1, { endpoint: name });
  if (response.status === 429) rejected429.add(1, { endpoint: name });
  if (response.status >= 500) serviceFailures.add(1, { endpoint: name });
  if (!valid && response.status < 500 && response.status !== 429) invalidAuthority.add(1, { endpoint: name });
  unexpectedErrors.add(response.status >= 500 || response.status === 0 || !valid && response.status !== 429, { endpoint: name });
}
export function setup() { const response = http.get(`${credentials.control}/pool`, { headers: { 'x-test-key': credentials.key }, responseType: 'text', tags: { name: 'test-control-setup', endpoint: 'control' } }); if (response.status !== 200) throw new Error('Initial authoritative token pool unavailable.'); return { tokens: response.json('tokens') }; }
export function introspection(data) {
  const measurementPhase = measured();
  const tokens = pool(data); const token = tokens[execution.scenario.iterationInTest % tokens.length]; const client = credentials.clients[token.client];
  const started = Date.now();
  const response = http.post(`${credentials.base}/oauth/introspect`, { token: token.access }, { headers: { Authorization: `Basic ${encoding(client.id, client.secret)}`, 'x-forwarded-for': source(execution.vu.idInTest), 'Content-Type': 'application/x-www-form-urlencoded' }, responseType: 'text', tags: { name: 'POST /oauth/introspect' } });
  const body = response.status === 200 ? response.json() : undefined;
  record('introspection', response, Date.now() - started, response.status === 200 && body.active === true && body.client_id === client.id, measurementPhase);
}
function encoding(id, secret) { return b64(`${id}:${secret}`); }
function b64(value) { const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'; let result = ''; for (let i = 0; i < value.length; i += 3) { const a = value.charCodeAt(i), b = value.charCodeAt(i + 1), c = value.charCodeAt(i + 2); const triple = a << 16 | (Number.isNaN(b) ? 0 : b) << 8 | (Number.isNaN(c) ? 0 : c); result += alphabet[triple >> 18 & 63] + alphabet[triple >> 12 & 63] + (Number.isNaN(b) ? '=' : alphabet[triple >> 6 & 63]) + (Number.isNaN(c) ? '=' : alphabet[triple & 63]); } return result; }
export function account() { const measurementPhase = measured(); const session = credentials.sessions[execution.scenario.iterationInTest % credentials.sessions.length]; const started = Date.now(); const response = http.get(`${credentials.base}/api/v1/me`, { headers: { Cookie: `identity-dev=${session.cookie}`, 'x-forwarded-for': source(execution.vu.idInTest) }, responseType: 'text', tags: { name: 'GET /api/v1/me' } }); const body = response.status === 200 ? response.json() : undefined; record('account', response, Date.now() - started, response.status === 200 && body.user.email_verified === true && body.session.current === true, measurementPhase); }
export function password() {
  const measurementPhase = measured();
  const index = execution.scenario.iterationInTest % credentials.emails.length;
  const started = Date.now(); const headers = { 'x-forwarded-for': source(index) };
  http.cookieJar().clear(credentials.base);
  const csrf = http.get(`${credentials.base}/api/v1/auth/csrf`, { headers, responseType: 'text', tags: { name: 'GET /api/v1/auth/csrf' } });
  if (csrf.status !== 200) { record('password', csrf, Date.now() - started, false, measurementPhase); return; }
  const response = http.post(`${credentials.base}/api/v1/auth/login/password`, JSON.stringify({ email: credentials.emails[index], password: credentials.password }), { headers: { ...headers, Origin: credentials.origin, 'X-CSRF-Token': csrf.json('csrf_token'), 'Content-Type': 'application/json' }, responseType: 'text', tags: { name: 'POST /api/v1/auth/login/password' } });
  const body = response.status === 200 ? response.json() : undefined; if (measurementPhase) passwordEndpointDuration.add(response.timings.duration); record('password', response, Date.now() - started, response.status === 200 && body.status === 'authenticated', measurementPhase);
}
export function handleSummary(data) {
  return { [__ENV.T21_SUMMARY]: JSON.stringify({ scenario: selected, warmupSeconds: 120, measurementSeconds: 900, rates, thresholds, metrics: data.metrics, state: data.state, note: 'Aggregate timing only. Raw HTTP bodies, credentials and token values deliberately excluded.' }, null, 2) };
}
