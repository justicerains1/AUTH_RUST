import { test, expect } from '@playwright/test';

test('@T06 password login grants real account access and logout clears it', async ({ page }) => {
  const password = process.env.T06_BROWSER_PASSWORD;
  if (!password) throw new Error('Private browser fixture password is required.');
  await page.goto('/login');
  await page.getByLabel('邮箱地址').fill('browser-session@example.test');
  await page.getByLabel('密码', { exact: true }).fill(password);
  await page.getByRole('button', { name: '登录', exact: true }).click();
  await expect(page).toHaveURL(/\/me$/u);
  await expect(page.getByText('browser-session@example.test', { exact: true }).first()).toBeVisible();
  const stored = await page.evaluate(() => ({ local: Object.keys(localStorage), session: Object.keys(sessionStorage) }));
  expect(stored).toEqual({ local: [], session: [] });
  await page.getByRole('button', { name: '退出当前设备', exact: true }).click();
  await page.getByRole('dialog').getByRole('button', { name: '确认退出', exact: true }).click();
  await expect(page).toHaveURL(/\/login$/u);
  await page.goto('/me');
  await expect(page).toHaveURL(/\/login$/u);
});

test('@T06 password login for MFA account does not bypass second factor', async ({ page }) => {
  const password = process.env.T06_BROWSER_PASSWORD;
  if (!password) throw new Error('Private browser fixture password is required.');
  await page.goto('/login');
  await page.getByLabel('邮箱地址').fill('browser-mfa@example.test');
  await page.getByLabel('密码', { exact: true }).fill(password);
  await page.getByRole('button', { name: '登录', exact: true }).click();
  await expect(page.getByRole('status').filter({ hasText: '还需要完成第二因素验证' })).toBeVisible();
  await page.goto('/me');
  await expect(page).toHaveURL(/\/login$/u);
});
