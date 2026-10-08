import { test, expect, type Page } from '@playwright/test';
async function appLogin(page: Page, port: number, password: boolean, email = 'browser-bff@example.test') {
  await page.goto(`http://localhost:${String(port)}/`);
  await page.getByRole('link', { name: '通过身份中心登录', exact: true }).click();
  if (password) { const secret = process.env.T13_BROWSER_PASSWORD; if (!secret) throw new Error('Private test password required.'); await page.getByLabel('邮箱地址').fill(email); await page.getByLabel('密码', { exact: true }).fill(secret); await page.getByRole('button', { name: '登录', exact: true }).click(); }
  await expect(page).toHaveURL(/\/oauth\/consent\/[0-9a-f-]{36}$/u);
  await page.getByRole('button', { name: '同意授权', exact: true }).click();
  await page.waitForURL((url) => url.port === String(port) && url.pathname === '/');
  await expect(page.getByText('已登录本应用', { exact: true })).toBeVisible();
}

test('@T13 A/B use the real shared identity with separate consent and browser-private tokens', async ({ page }) => {
  let forbiddenNetwork = false;
  page.on('request', (request) => { const url = new URL(request.url()); if (url.pathname === '/oauth/token' || /client_secret|access_token|refresh_token|id_token/u.test(request.postData() ?? '')) forbiddenNetwork = true; });
  await appLogin(page, 5192, true); await appLogin(page, 5193, false);
  expect(forbiddenNetwork).toBe(false);
  const body = await page.evaluate(async () => { const response = await fetch('/bff/session', { credentials: 'same-origin' }); if (!response.ok) throw new Error('Real BFF session was not available.'); return response.json() as Promise<unknown>; });
  expect(JSON.stringify(body).match(/access_token|refresh_token|id_token|client_secret/u)).toBeNull();
  const cookies = await page.context().cookies(); expect(cookies.some((cookie) => cookie.name === 't13-a-session')).toBe(true); expect(cookies.some((cookie) => cookie.name === 't13-b-session')).toBe(true);
  const stored = await page.evaluate(() => ({ local: Object.keys(localStorage), session: Object.keys(sessionStorage) })); expect(stored).toEqual({ local: [], session: [] });
  await page.goto('http://localhost:5190/me'); await page.getByRole('button', { name: '退出所有设备', exact: true }).click(); await page.getByRole('dialog').getByRole('button', { name: '退出所有设备', exact: true }).click(); await expect(page).toHaveURL(/\/login$/u);
  await page.goto('http://localhost:5192/'); await expect(page.getByRole('link', { name: '通过身份中心登录', exact: true })).toBeVisible(); await page.goto('http://localhost:5193/'); await expect(page.getByRole('link', { name: '通过身份中心登录', exact: true })).toBeVisible();
});

test('@T13 concurrent requests across two BFF instances share one refresh lock', async ({ page }) => {
  await appLogin(page, 5192, true, 'browser-bff-race@example.test');
  const key = process.env.T13_TEST_KEY; if (!key) throw new Error('Private test key required.');
  const before = await (await fetch('http://127.0.0.1:5197/__test/refresh', { method: 'POST', headers: { 'x-test-key': key } })).json() as { prepared: number; refresh_count: number }; expect(before.prepared > 0).toBe(true);
  const cookie = (await page.context().cookies()).find((item) => item.name === 't13-a-session'); if (!cookie) throw new Error('Private BFF session cookie unavailable.');
  const requests = Array.from({ length: 10 }, (_, index) => fetch(`http://127.0.0.1:${String(index % 2 ? 5194 : 5196)}/bff/session`, { headers: { Cookie: `t13-a-session=${cookie.value}` } }));
  const responses = await Promise.all(requests); expect(responses.every((response) => response.status === 200)).toBe(true);
  const after = await (await fetch('http://127.0.0.1:5197/__test/refresh', { method: 'POST', headers: { 'x-test-key': key } })).json() as { refresh_count: number };
  expect(after.refresh_count - before.refresh_count).toBe(1);
});

test('@T13 identity-state failure returns503 and local logout only affects its application', async ({ page, browser }) => {
  await appLogin(page, 5192, true, 'browser-bff-fault@example.test'); await appLogin(page, 5193, false, 'browser-bff-fault@example.test');
  const cookies = await page.context().cookies(); const a = cookies.find((item) => item.name === 't13-a-session'); const b = cookies.find((item) => item.name === 't13-b-session'); if (!a || !b) throw new Error('Private application sessions missing.');
  const { spawn } = await import('node:child_process');
  const path = await import('node:path');
  const composeCommand = (args: string[]) => new Promise<string>((resolveDone, reject) => { const child = spawn('docker', ['compose', '--env-file', path.resolve('.local/dev.env'), '-f', path.resolve('infra/compose.dev.yaml'), ...args], { stdio: ['ignore', 'pipe', 'ignore'], shell: false }); let output = ''; child.stdout.setEncoding('utf8'); child.stdout.on('data', (value: string) => { output += value; }); child.on('error', () => { reject(new Error('Dependency command unavailable.')); }); child.on('close', (code) => { if (code === 0) resolveDone(output); else reject(new Error('Dependency command failed.')); }); });
  async function compose(action: 'stop' | 'start') {
    await composeCommand([action, 'redis']);
    if (action === 'start') {
      // Starting the container does not imply Redis is ready for the next real authorization.
      await expect.poll(async () => { try { return (await composeCommand(['exec', '-T', 'redis', 'redis-cli', 'PING'])).trim() === 'PONG'; } catch { return false; } }, { timeout: 15_000 }).toBe(true);
    }
  }
  await compose('stop');
  try { const response = await fetch('http://127.0.0.1:5194/bff/session', { headers: { Cookie: `t13-a-session=${a.value}` } }); expect(response.status).toBe(503); }
  finally { await compose('start'); }
  await expect.poll(async () => (await fetch('http://127.0.0.1:5194/bff/session', { headers: { Cookie: `t13-a-session=${a.value}` } })).status).toBe(200);
  await page.goto('http://localhost:5192/'); await page.getByRole('button', { name: '退出本应用', exact: true }).click(); await expect(page.getByRole('link', { name: '通过身份中心登录', exact: true })).toBeVisible();
  const unaffected = await fetch('http://127.0.0.1:5195/bff/session', { headers: { Cookie: `t13-b-session=${b.value}` } }); expect(unaffected.status).toBe(200);
  const key = process.env.T13_TEST_KEY; if (!key) throw new Error('Private test key missing.');
  const grants = await (await fetch('http://127.0.0.1:5197/__test/grants', { headers: { 'x-test-key': key } })).json() as { a: number; b: number };
  expect(grants.a).toBe(0); expect(grants.b > 0).toBe(true);
  await compose('stop');
  try {
    await page.goto('http://localhost:5193/');
    const csrf = await page.evaluate(async () => (await (await fetch('/bff/csrf', { credentials: 'same-origin' })).json()) as { csrf_token: string });
    const response = await page.evaluate(async (value) => {
      const response = await fetch('/bff/logout', { method: 'POST', credentials: 'same-origin', headers: { 'X-CSRF-Token': value } });
      const body = await response.json() as { revocation_status: string }; return { status: response.status, failed: body.revocation_status === 'failed' };
    }, csrf.csrf_token);
    expect(response.status).toBe(200); expect(response.failed).toBe(true);
    const privateCookies = await page.context().cookies(); expect(privateCookies.some((item) => item.name === 't13-b-session')).toBe(false);
  } finally { await compose('start'); }
  const refreshContext = await browser.newContext(); const refreshPage = await refreshContext.newPage();
  try {
    await appLogin(refreshPage, 5192, true, 'browser-bff-refresh-fault@example.test');
    await fetch('http://127.0.0.1:5197/__test/refresh', { method: 'POST', headers: { 'x-test-key': key } });
    await compose('stop');
    try {
      const status = await refreshPage.evaluate(async () => (await fetch('/bff/session', { credentials: 'same-origin' })).status); expect(status).toBe(503);
      const after = await refreshContext.cookies(); expect(after.some((item) => item.name === 't13-a-session')).toBe(false);
    } finally { await compose('start'); }
  } finally { await refreshContext.close(); }
});

test('@T13 callback state tampering is rejected and platform logout waits for identity confirmation', async ({ page, browser }) => {
  const password = process.env.T13_BROWSER_PASSWORD; if (!password) throw new Error('Private test password required.');
  await page.goto('http://localhost:5192/bff/login');
  await page.getByLabel('邮箱地址').fill('browser-bff-negative@example.test'); await page.getByLabel('密码', { exact: true }).fill(password); await page.getByRole('button', { name: '登录', exact: true }).click();
  await expect(page).toHaveURL(/\/oauth\/consent\/[0-9a-f-]{36}$/u);
  await page.route('http://localhost:5192/bff/callback?**', async (route) => { const url = new URL(route.request().url()); url.searchParams.set('state', 'T13_WRONG_STATE'); await route.continue({ url: url.toString() }); });
  await page.getByRole('button', { name: '同意授权', exact: true }).click(); await page.waitForURL((url) => url.pathname === '/bff/callback');
  const rejected = await page.evaluate(async () => (await fetch('/bff/session', { credentials: 'same-origin' })).status); expect(rejected).toBe(401);
  await page.unroute('http://localhost:5192/bff/callback?**');
  const clean = await browser.newContext(); const platform = await clean.newPage();
  try {
    await appLogin(platform, 5192, true, 'browser-bff-platform@example.test');
    await platform.getByRole('button', { name: '退出身份平台', exact: true }).click(); await expect(platform).toHaveURL(/\/logout\?confirmation=[0-9a-f-]{36}$/u);
    await platform.getByRole('button', { name: '确认退出', exact: true }).click(); await expect(platform).toHaveURL(/\/login$/u);
    await platform.goto('http://localhost:5192/'); await expect(platform.getByRole('link', { name: '通过身份中心登录', exact: true })).toBeVisible();
  } finally { await clean.close(); }
});
