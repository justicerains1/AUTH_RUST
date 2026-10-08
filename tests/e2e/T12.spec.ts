import { test, expect, type Page } from '@playwright/test';
async function login(page: Page) { const password = process.env.T12_BROWSER_PASSWORD; if (!password) throw new Error('Private test password required.'); await page.goto('/login'); await page.getByLabel('邮箱地址').fill('browser-logout@example.test'); await page.getByLabel('密码', { exact: true }).fill(password); await page.getByRole('button', { name: '登录', exact: true }).click(); await expect(page).toHaveURL(/\/me$/u); }
async function grant(page: Page) { const params = new URLSearchParams({ client_id: 't12-a', response_type: 'code', redirect_uri: 'http://localhost:5190/callback', scope: 'openid email', state: 'T12_BROWSER_STATE_12345', nonce: 'T12_BROWSER_NONCE_12345', code_challenge: 'A'.repeat(43), code_challenge_method: 'S256', prompt: 'consent' }); await page.goto(`/oauth/authorize?${params}`); await expect(page).toHaveURL(/\/oauth\/consent\/[0-9a-f-]{36}$/u); await page.getByRole('button', { name: '同意授权', exact: true }).click(); await page.waitForURL((url) => url.pathname === '/callback'); }

test('@T12 account grants can be revoked through an explicit confirmation', async ({ page }) => {
  await login(page); await grant(page); await page.goto('/me/grants'); await page.getByRole('button', { name: '撤销授权', exact: true }).first().click(); await page.getByRole('dialog').getByRole('button', { name: '确认撤销', exact: true }).click(); await expect(page.getByText('还没有授权应用。', { exact: true })).toBeVisible();
});
test('@T12 RP logout entry waits for confirmation and supports cancellation', async ({ page }) => {
  await login(page); await page.goto('/oauth/logout'); await expect(page).toHaveURL(/\/logout\?confirmation=[0-9a-f-]{36}$/u); await page.getByRole('button', { name: '取消', exact: true }).click(); await page.goto('/me'); await expect(page).toHaveURL(/\/me$/u);
  await page.goto('/oauth/logout'); await page.getByRole('button', { name: '确认退出', exact: true }).click(); await expect(page).toHaveURL(/\/login$/u); await page.goto('/me'); await expect(page).toHaveURL(/\/login$/u);
});
