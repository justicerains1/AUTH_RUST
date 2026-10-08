/* global __ENV, open */
import http from 'k6/http';
import encoding from 'k6/encoding';
import execution from 'k6/execution';
import { Counter, Rate, Trend } from 'k6/metrics';

// A separate short staircase experiment. Baseline fifteen-minute scenarios stay unchanged.
const credentials = JSON.parse(open(__ENV.T21_CREDENTIALS));
if (__ENV.APP_ENV !== 'test' || credentials.base !== 'http://127.0.0.1:5310' || credentials.control !== 'http://127.0.0.1:5311' || credentials.clients.length !== 20 || credentials.sessions.length !== 2048) throw new Error('Explicit isolated capacity fixture required.');
const rates = { introspection: 300, account: 50, password: 5 };
const multipliers = [1, 2, 4, 8];
const scenarios = {};
const thresholds = {};
const counts = new Counter('capacity_requests');
const durations = new Trend('capacity_duration', true);
const errors = new Rate('capacity_unexpected_errors');
const limited = new Counter('capacity_rate_limited');
const invalid = new Counter('capacity_invalid_authority');
for (const multiplier of multipliers) {
  for (const [endpoint, rate] of Object.entries(rates)) {
    const name = `${endpoint}_${String(multiplier)}`;
    scenarios[name] = { executor: 'constant-arrival-rate', exec: endpoint, startTime: `${String(multipliers.indexOf(multiplier) * 120)}s`, rate: rate * multiplier, timeUnit: '1s', duration: '120s', gracefulStop: '5s', preAllocatedVUs: endpoint === 'password' ? 32 : 128, maxVUs: endpoint === 'password' ? 128 : 512, tags: { endpoint, multiplier: String(multiplier) } };
    const tags = `endpoint:${endpoint},multiplier:${String(multiplier)}`;
    thresholds[`capacity_requests{${tags}}`] = [`count>=${String(rate * multiplier * 60)}`];
    thresholds[`capacity_unexpected_errors{${tags}}`] = ['rate<0.001'];
    thresholds[`capacity_duration{${tags}}`] = [`p(95)<=${endpoint === 'password' ? '1000' : endpoint === 'account' ? '200' : '100'}`];
  }
}
thresholds.dropped_iterations = ['count==0'];
export const options = { scenarios, thresholds, discardResponseBodies: true, summaryTrendStats: ['avg', 'min', 'med', 'max', 'p(95)', 'p(99)'], systemTags: ['status', 'method', 'name', 'scenario', 'error_code'], userAgent: 'Identity isolated capacity measurement', insecureSkipTLSVerify: false };
let tokenPool;
let updated = 0;
function measured() { const elapsed = Date.now() - execution.scenario.startTime; return elapsed >= 60_000 && elapsed < 120_000; }
function tags(endpoint) { return { endpoint, multiplier: execution.scenario.name.split('_').at(-1) }; }
function source(index) { return `198.20.${Math.floor(index / 250) % 250}.${index % 250 + 1}`; }
function current() {
  // Later staircase scenarios start minutes after setup; its original tokens may already expire.
  // Fetch the current pool for each newly started VU instead of treating setup's age as zero.
  if (!tokenPool || Date.now() - updated >= 60_000) {
    const result = http.get(`${credentials.control}/pool`, { headers: { 'x-test-key': credentials.key }, responseType: 'text', tags: { name: 'capacity-control-pool' } });
    if (result.status !== 200) throw new Error('Authoritative capacity token pool unavailable.');
    tokenPool = result.json('tokens'); updated = Date.now();
  }
  return tokenPool;
}
function record(endpoint, response, elapsed, valid, phase) {
  if (!phase) return;
  const labels = tags(endpoint);
  counts.add(1, labels); durations.add(elapsed, labels);
  limited.add(response.status === 429 ? 1 : 0, labels);
  invalid.add(!valid && response.status > 0 && response.status < 500 && response.status !== 429 ? 1 : 0, labels);
  // Normal load rejection is a capacity limit, not a successful measurement.
  errors.add(response.status === 0 || response.status >= 500 || response.status === 429 || !valid, labels);
}
export function setup() {
  const result = http.get(`${credentials.control}/pool`, { headers: { 'x-test-key': credentials.key }, responseType: 'text', tags: { name: 'capacity-control-setup' } });
  if (result.status !== 200) throw new Error('Initial authoritative capacity pool unavailable.');
  return { tokens: result.json('tokens') };
}
export function introspection() {
  const phase = measured(); const pool = current(); const token = pool[execution.scenario.iterationInTest % pool.length]; const client = credentials.clients[token.client]; const start = Date.now();
  const result = http.post(`${credentials.base}/oauth/introspect`, { token: token.access }, { headers: { Authorization: `Basic ${encoding.b64encode(`${client.id}:${client.secret}`)}`, 'x-forwarded-for': source(execution.vu.idInTest), 'Content-Type': 'application/x-www-form-urlencoded' }, responseType: 'text', tags: { name: 'capacity POST /oauth/introspect' } });
  const body = result.status === 200 ? result.json() : null;
  record('introspection', result, Date.now() - start, result.status === 200 && body?.active === true && body.client_id === client.id, phase);
}
export function account() {
  const phase = measured(); const session = credentials.sessions[execution.scenario.iterationInTest % credentials.sessions.length]; const start = Date.now();
  const result = http.get(`${credentials.base}/api/v1/me`, { headers: { Cookie: `identity-dev=${session.cookie}`, 'x-forwarded-for': source(execution.vu.idInTest) }, responseType: 'text', tags: { name: 'capacity GET /api/v1/me' } });
  const body = result.status === 200 ? result.json() : null;
  record('account', result, Date.now() - start, result.status === 200 && body?.user.email_verified === true && body.session.current === true, phase);
}
export function password() {
  const phase = measured(); const index = execution.scenario.iterationInTest % credentials.emails.length; const start = Date.now(); const headers = { 'x-forwarded-for': source(index) };
  http.cookieJar().clear(credentials.base);
  const csrf = http.get(`${credentials.base}/api/v1/auth/csrf`, { headers, responseType: 'text', tags: { name: 'capacity GET /api/v1/auth/csrf' } });
  if (csrf.status !== 200) { record('password', csrf, Date.now() - start, false, phase); return; }
  const result = http.post(`${credentials.base}/api/v1/auth/login/password`, JSON.stringify({ email: credentials.emails[index], password: credentials.password }), { headers: { ...headers, Origin: credentials.origin, 'X-CSRF-Token': csrf.json('csrf_token'), 'Content-Type': 'application/json' }, responseType: 'text', tags: { name: 'capacity POST /api/v1/auth/login/password' } });
  record('password', result, Date.now() - start, result.status === 200 && result.json('status') === 'authenticated', phase);
}
export function handleSummary(data) {
  return { [__ENV.T21_SUMMARY]: JSON.stringify({ kind: 'Short local capacity staircase, not reference or fifteen-minute acceptance', multipliers, baseRates: rates, warmupSecondsPerStage: 60, measurementSecondsPerStage: 60, metrics: data.metrics, thresholds, state: data.state, note: 'Failed thresholds preserve the measured limit. No raw responses or identity credentials.' }, null, 2) };
}
