// T21.07 frontend measurements only: production files, actual loopback API, no authentication mocks.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { readFile, readdir, mkdir, writeFile } from 'node:fs/promises';
import { createServer, request as httpRequest } from 'node:http';
import { cpus, totalmem, platform, release, arch } from 'node:os';
import { resolve, relative, extname, sep } from 'node:path';
import { gzipSync } from 'node:zlib';
import { chromium } from 'playwright';

const root = resolve(import.meta.dirname, '../..');
const dist = resolve(root, 'apps/identity-web/dist');
const evidence = resolve(root, 'docs/evidence/T21');
const runId = new Date().toISOString().replaceAll(':', '-').replaceAll('.', '-');
const api = new URL(process.env.T21_FRONTEND_API ?? 'http://127.0.0.1:8080');
assert.ok(api.protocol === 'http:' && ['localhost', '127.0.0.1', '[::1]'].includes(api.hostname) && api.username === '' && api.password === '' && api.pathname === '/' && api.search === '' && api.hash === '', 'Frontend performance requires an explicit loopback HTTP API origin without credentials.');
assert.ok(process.argv.length === 2, 'Usage: node tests/performance/frontend.mjs');
const expectedChrome = 'HeadlessChrome/153.0.8010.12';
const settings = { expectedChrome, viewport: { width: 390, height: 844 }, samples: 5, cpuSlowdown: 4, downlinkMbps: 1.6, uplinkKbps: 750, latencyMs: 150, downloadBytesPerSecond: 200000, uploadBytesPerSecond: 93750 };
const limits = { lcpMs: 2500, cls: 0.1, homeJavaScriptGzipBytes: 300 * 1024, loginJavaScriptGzipBytes: 200 * 1024, localFeedbackMaxMs: 100, laboratoryFeedbackP75Ms: 200 };
const report = { kind: 'T21 frontend independent submodule; does not mark T21 accepted', started: new Date().toISOString(), environment: {}, settings, limits, methodology: { build: 'Existing production dist; source and every output SHA-256 recorded', network: 'CDP emulation; static server supplies gzip bytes; actual GET /api/v1/me proxies to fixed loopback API', cache: 'New incognito context for every cold sample; browser HTTP cache cleared and disabled; service workers blocked', metrics: 'LCP latest observer entry before interaction; CLS maximum standard 1s-gap/5s session window without recent input; five-sample median', interaction: 'Each sample reloads to clear prior feedback; click event timestamp to second animation frame after password visibility/local empty-login feedback; local CPU1x and laboratory CPU4x measured separately, not INP or a Lighthouse score', scope: 'Home and anonymous login rendering only. No account seed, token generation, password login or production RUM.' }, build: [], coldLoads: [], interactions: [], summaries: [], assertions: [], failures: [] };
let server;
let browser;
let origin;
let stage = "setup";
const assets = new Map();
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
function percentile(values, fraction) { const sorted = [...values].sort((a, b) => a - b); return sorted[Math.max(0, Math.ceil(sorted.length * fraction) - 1)]; }
function median(values) { return percentile(values, 0.5); }
function check(name, actual, limit) { const passed = actual <= limit; report.assertions.push({ name, actual, limit, passed }); if (!passed) report.failures.push(`${name} exceeds its documented threshold.`); }
async function optionalFile(path) { try { return (await readFile(path, 'utf8')).trim(); } catch { return 'unavailable'; } }
async function files(directory) { const result = []; for (const entry of await readdir(directory, { withFileTypes: true })) { const path = resolve(directory, entry.name); if (entry.isDirectory()) result.push(...await files(path)); else if (entry.isFile()) result.push(path); } return result.sort(); }
async function startServer() {
  for (const path of await files(dist)) {
    const bytes = await readFile(path); const compressed = gzipSync(bytes);
    const pathname = `/${relative(dist, path).split(sep).join('/')}`;
    assets.set(pathname, { bytes, compressed });
    report.build.push({ path: pathname, bytes: bytes.length, gzipBytes: compressed.length, sha256: sha256(bytes) });
  }
  assert.ok(assets.has('/index.html'), 'Build the identity-web production files before measuring.');
  server = createServer((incoming, outgoing) => {
    const url = new URL(incoming.url ?? '/', 'http://measurement.invalid');
    if (url.pathname.startsWith('/api/')) {
      if (incoming.method !== 'GET' || url.pathname !== '/api/v1/me' || url.search !== '') { outgoing.writeHead(400); outgoing.end(); return; }
      const upstream = httpRequest(new URL('/api/v1/me', api), { method: 'GET', headers: { Accept: 'application/json' }, timeout: 3000 }, (response) => {
        outgoing.writeHead(response.statusCode ?? 503, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' }); response.pipe(outgoing);
      });
      upstream.on('error', () => { if (!outgoing.headersSent) outgoing.writeHead(503, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' }); outgoing.end(); }); upstream.on('timeout', () => upstream.destroy()); upstream.end(); return;
    }
    const entry = assets.get(url.pathname) ?? (extname(url.pathname) === '' ? assets.get('/index.html') : undefined);
    if (incoming.method !== 'GET' || entry === undefined) { outgoing.writeHead(404); outgoing.end(); return; }
    const gzip = incoming.headers['accept-encoding']?.includes('gzip') === true;
    const body = gzip ? entry.compressed : entry.bytes;
    const mime = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8', '.svg': 'image/svg+xml', '.png': 'image/png' }[extname(url.pathname)] ?? 'text/html; charset=utf-8';
    outgoing.writeHead(200, { 'Content-Type': mime, 'Content-Length': body.length, 'Cache-Control': 'no-store', Vary: 'Accept-Encoding', ...(gzip ? { 'Content-Encoding': 'gzip' } : {}) }); outgoing.end(body);
  });
  await new Promise((done) => { server.listen(0, '127.0.0.1', done); });
  const address = server.address(); assert.ok(address !== null && typeof address === 'object'); origin = `http://127.0.0.1:${address.port}`;
}
async function configure(context, page, slowdown, network) {
  const cdp = await context.newCDPSession(page);
  await cdp.send('Network.enable'); await cdp.send('Network.clearBrowserCache'); await cdp.send('Network.setCacheDisabled', { cacheDisabled: true });
  await cdp.send('Emulation.setCPUThrottlingRate', { rate: slowdown });
  if (network) await cdp.send('Network.emulateNetworkConditions', { offline: false, latency: settings.latencyMs, downloadThroughput: settings.downloadBytesPerSecond, uploadThroughput: settings.uploadBytesPerSecond, connectionType: 'cellular3g' });
  return cdp;
}
async function coldLoad(route, sample) {
  const context = await browser.newContext({ viewport: settings.viewport, serviceWorkers: 'block' });
  const page = await context.newPage();
  const cdp = await configure(context, page, 4, true);
  const loaded = new Set(); const failed = []; const apiStatuses = []; let mutations = 0;
  page.on('request', (request) => { if (request.method() !== 'GET') mutations += 1; });
  page.on('response', (response) => { const url = new URL(response.url()); if (url.origin === origin && url.pathname.endsWith('.js')) loaded.add(url.pathname); if (url.pathname === '/api/v1/me') apiStatuses.push(response.status()); });
  page.on('pageerror', () => { failed.push('Browser JavaScript exception.'); });
  page.on('requestfailed', (request) => { if (request.url().startsWith(origin)) failed.push(`Required page resource did not complete: ${new URL(request.url()).pathname}`); });
  await page.addInitScript(() => {
    globalThis.__frontendPerformance = { lcp: [], shifts: [] };
    new PerformanceObserver((list) => { for (const entry of list.getEntries()) globalThis.__frontendPerformance.lcp.push({ startTime: entry.startTime, size: entry.size, element: entry.element?.tagName ?? null }); }).observe({ type: 'largest-contentful-paint', buffered: true });
    new PerformanceObserver((list) => { for (const entry of list.getEntries()) if (!entry.hadRecentInput) globalThis.__frontendPerformance.shifts.push({ startTime: entry.startTime, value: entry.value }); }).observe({ type: 'layout-shift', buffered: true });
  });
  try {
    stage = `cold ${route.path} sample ${sample}: navigation`;
    await page.goto(`${origin}${route.path}`, { waitUntil: 'networkidle', timeout: 45000 });
    stage = `cold ${route.path} sample ${sample}: real heading`;
    await page.getByRole('heading', { name: route.heading, level: 1, exact: true }).waitFor();
    if (route.path === '/login') { await page.getByLabel('密码', { exact: true }).waitFor(); assert.deepEqual(apiStatuses, [401], 'Login rendering must use an actual anonymous API 401, not an unavailable or mocked response.'); }
    await page.waitForTimeout(1000);
    const data = await page.evaluate(() => {
      const metrics = globalThis.__frontendPerformance;
      let max = 0, value = 0, start = -Infinity, last = -Infinity;
      for (const shift of metrics.shifts) { if (shift.startTime - last >= 1000 || shift.startTime - start >= 5000) { value = 0; start = shift.startTime; } value += shift.value; last = shift.startTime; max = Math.max(max, value); }
      const lastLcp = metrics.lcp.at(-1);
      return { lcpMs: lastLcp?.startTime ?? null, lcpElement: lastLcp?.element ?? null, cls: max, lcpEntries: metrics.lcp, layoutShifts: metrics.shifts, overflow: globalThis.document.documentElement.scrollWidth > globalThis.innerWidth };
    });
    stage = `cold ${route.path} sample ${sample}: metrics`;
    assert.ok(data.lcpMs !== null, 'A real LCP entry is required.'); assert.equal(mutations, 0, 'Rendering measurements must not submit authentication.'); assert.deepEqual(failed, [], 'Rendering or required network resources failed.');
    const resources = [...loaded].sort().map((path) => { const asset = assets.get(path); assert.ok(asset !== undefined, 'A requested JavaScript file is missing from the recorded build.'); return { path, bytes: asset.bytes.length, gzipBytes: asset.compressed.length }; });
    report.coldLoads.push({ route: route.path, sample, ...data, javaScriptGzipBytes: resources.reduce((total, asset) => total + asset.gzipBytes, 0), resources, actualAnonymousApiStatuses: apiStatuses, mutations });
  } catch (error) { report.diagnostics = { stage, pageErrors: failed, headingCount: await page.locator('h1').count(), headings: await page.locator('h1').allTextContents(), documentTitle: await page.title(), resourcePaths: [...loaded].sort() }; throw error; } finally { await cdp.detach(); await context.close(); }
}
async function feedback(page, locator, resultSelector) {
  await page.evaluate(({ button, result }) => {
    const target = globalThis.document.querySelector(button); if (target === null) throw new Error('Interaction target unavailable.');
    globalThis.__feedbackSample = new Promise((resolveResult) => {
      target.addEventListener('click', (event) => {
        const started = event.timeStamp;
        globalThis.requestAnimationFrame(() => { globalThis.requestAnimationFrame(() => { resolveResult({ elapsedMs: performance.now() - started, feedbackVisible: globalThis.document.querySelector(result) !== null }); }); });
      }, { once: true, capture: true });
    });
  }, { button: resultSelector.button, result: resultSelector.result });
  await locator.click();
  return page.evaluate(() => globalThis.__feedbackSample);
}
async function interactions(slowdown, label) {
  const context = await browser.newContext({ viewport: settings.viewport, serviceWorkers: 'block' }); const page = await context.newPage(); const cdp = await configure(context, page, slowdown, label === 'laboratory');
  let mutations = 0; page.on('request', (request) => { if (request.method() !== 'GET') mutations += 1; });
  try {
    await page.goto(`${origin}/login`, { waitUntil: 'networkidle' }); await page.getByRole('heading', { name: '登录', exact: true }).waitFor();
    for (let sample = 1; sample <= 5; sample += 1) {
      if (sample > 1) { await page.reload({ waitUntil: 'networkidle' }); await page.getByLabel('密码', { exact: true }).waitFor(); }
      assert.equal(await page.locator('input[name="email"][aria-invalid="true"]').count(), 0, 'Each feedback sample must begin without an existing validation result.');
      const before = await page.getByLabel('密码', { exact: true }).getAttribute('type');
      const selector = before === 'password' ? 'input[name="password"][type="text"]' : 'input[name="password"][type="password"]';
      const toggle = await feedback(page, page.getByRole('button', { name: before === 'password' ? '显示密码' : '隐藏密码', exact: true }), { button: '.password-toggle', result: selector });
      const validate = await feedback(page, page.getByRole('button', { name: '登录', exact: true }), { button: 'button[type="submit"]', result: 'input[name="email"][aria-invalid="true"]' });
      assert.ok(toggle.feedbackVisible && validate.feedbackVisible, 'Measured interaction must produce the expected real local UI feedback.');
      report.interactions.push({ mode: label, cpuSlowdown: slowdown, sample, passwordVisibilityMs: toggle.elapsedMs, localValidationMs: validate.elapsedMs });
    }
    assert.equal(mutations, 0, 'Empty-form local interaction measurement must never submit authentication.');
  } finally { await cdp.detach(); await context.close(); }
}
async function main() {
  const cpu = cpus(); const git = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim();
  report.environment = { os: `${platform()} ${release()} ${arch()}`, node: process.version, cpuModel: cpu[0]?.model ?? 'unknown', logicalCpus: cpu.length, memoryGiB: totalmem() / 1024 ** 3, cgroupCpuMax: await optionalFile('/sys/fs/cgroup/cpu.max'), cgroupMemoryMax: await optionalFile('/sys/fs/cgroup/memory.max'), referenceEnvironment: 'Linux 8 vCPU / 16 GiB / SSD; this host differs, backend capacity is not measured.', git, workspaceHasChanges: execFileSync('git', ['status', '--porcelain'], { cwd: root, encoding: 'utf8' }).length > 0 };
  for (const directory of ['apps/identity-web/src']) for (const path of await files(resolve(root, directory))) report.build.push({ source: relative(root, path).split(sep).join('/'), sha256: sha256(await readFile(path)) });
  await startServer(); browser = await chromium.launch({ headless: true });
  const context = await browser.newContext(); const page = await context.newPage(); const cdp = await context.newCDPSession(page); const version = await cdp.send('Browser.getVersion'); report.environment.chrome = version; assert.equal(version.product, expectedChrome, 'Chrome version differs from this fixed experiment; update the measurement protocol explicitly before changing the baseline.'); report.environment.browserExecutableSha256 = sha256(await readFile(chromium.executablePath())); await cdp.detach(); await context.close();
  for (const route of [{ path: '/', heading: /一个身份/u }, { path: '/login', heading: '登录' }]) {
    for (let sample = 1; sample <= 5; sample += 1) await coldLoad(route, sample);
    const samples = report.coldLoads.filter((sample) => sample.route === route.path); const summary = { route: route.path, medianLcpMs: median(samples.map((sample) => sample.lcpMs)), medianCls: median(samples.map((sample) => sample.cls)), maximumJavaScriptGzipBytes: Math.max(...samples.map((sample) => sample.javaScriptGzipBytes)), horizontalOverflow: samples.some((sample) => sample.overflow) }; report.summaries.push(summary);
    check(`${route.path} median LCP`, summary.medianLcpMs, limits.lcpMs); check(`${route.path} median CLS`, summary.medianCls, limits.cls); check(`${route.path} first-screen JavaScript gzip`, summary.maximumJavaScriptGzipBytes, route.path === '/' ? limits.homeJavaScriptGzipBytes : limits.loginJavaScriptGzipBytes); if (summary.horizontalOverflow) report.failures.push(`${route.path} has horizontal overflow.`);
  }
  await interactions(1, 'local'); await interactions(4, 'laboratory');
  for (const mode of ['local', 'laboratory']) { const samples = report.interactions.filter((sample) => sample.mode === mode); for (const metric of ['passwordVisibilityMs', 'localValidationMs']) { const values = samples.map((sample) => sample[metric]); const value = mode === 'local' ? Math.max(...values) : percentile(values, 0.75); check(`${mode} ${metric} ${mode === 'local' ? 'maximum' : 'p75'}`, value, mode === 'local' ? limits.localFeedbackMaxMs : limits.laboratoryFeedbackP75Ms); } }
}
try { await main(); } catch (error) { report.failureStage = stage; report.failureType = error.name; if (error instanceof assert.AssertionError) report.failures.push(error.message.split('\n')[0]); report.failures.push('Measurement could not complete; check the production build, fixed local API, and Chrome installation. No secret-bearing transport diagnostics are retained.'); }
finally {
  report.completed = new Date().toISOString(); report.exitCode = report.failures.length === 0 ? 0 : 1;
  await browser?.close(); await new Promise((done) => { if (server !== undefined) server.close(done); else done(); });
  await mkdir(evidence, { recursive: true }); const path = resolve(evidence, `frontend-${runId}${report.exitCode === 0 ? '' : '-failure'}.json`); await writeFile(path, `${JSON.stringify(report, null, 2)}\n`, { flag: 'wx' });
  console.log(`Frontend measurement ${report.exitCode === 0 ? 'met' : 'did not meet'} this submodule's thresholds. Raw report: ${relative(root, path)}. T21 overall acceptance remains unchanged.`);
  process.exitCode = report.exitCode;
}
