import { test, expect, type Page } from '@playwright/test';
function authorize(prompt?: string): string {
  const params = new URLSearchParams({ client_id: 't10-a', response_type: 'code', redirect_uri: 'http://localhost:5190/callback', scope: 'openid email', state: 'T10_BROWSER_STATE_FIXTURE_173', nonce: 'T10_BROWSER_NONCE_FIXTURE_283', code_challenge: 'A'.repeat(43), code_challenge_method: 'S256', ...(prompt ? { prompt } : {}) });
  return `http://localhost:5190/oauth/authorize?${params}`;
}
async function login(page: Page) {
  const password = process.env.T10_BROWSER_PASSWORD; if (!password) throw new Error('Private browser password required.');
  await page.getByLabel('邮箱地址').fill('browser-oauth@example.test'); await page.getByLabel('密码', { exact: true }).fill(password); await page.getByRole('button', { name: '登录', exact: true }).click();
  await expect(page).toHaveURL(/\/oauth\/consent\/[0-9a-f-]{36}$/u);
}

test('@T10 real authorize login continuation and consent produce a registered callback code', async ({ page }) => {
  await page.goto(authorize()); await expect(page).toHaveURL(/\/login\?transaction=[0-9a-f-]{36}$/u);
  await login(page); await expect(page.getByText('T10 Example App A', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: '同意授权', exact: true }).click();
  await page.waitForURL((url) => url.pathname === '/callback');
  const callback = new URL(page.url()); expect(callback.origin).toBe('http://localhost:5190'); expect(callback.searchParams.has('code')).toBe(true); expect(callback.searchParams.get('state') === 'T10_BROWSER_STATE_FIXTURE_173').toBe(true);
});

test('@T10 explicit denial returns standard error at the registered callback', async ({ page }) => {
  await page.goto(authorize('consent')); await login(page); await page.getByRole('button', { name: '拒绝授权', exact: true }).click();
  await page.waitForURL((url) => url.pathname === '/callback'); const callback = new URL(page.url()); expect(callback.searchParams.get('error')).toBe('access_denied'); expect(callback.searchParams.get('state') === 'T10_BROWSER_STATE_FIXTURE_173').toBe(true); expect(callback.searchParams.has('code')).toBe(false);
});
