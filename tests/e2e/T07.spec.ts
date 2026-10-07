import { test, expect, type Page } from '@playwright/test';
async function resetToken(email: string): Promise<string> {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    const list = await (await fetch('http://127.0.0.1:8025/api/v1/messages')).json() as { messages: { ID: string; To: { Address: string }[] }[] };
    for (const mail of list.messages) {
      if (!mail.To.some((to) => to.Address === email)) continue;
      const detail = await (await fetch(`http://127.0.0.1:8025/api/v1/message/${mail.ID}`)).json() as { Text: string };
      const token = /\/password-reset#token=([A-Za-z0-9_-]{43})/u.exec(detail.Text)?.[1];
      if (token) return token;
    }
    await new Promise((resolveDelay) => setTimeout(resolveDelay, 100));
  }
  throw new Error('Password-reset mail was not delivered.');
}
async function login(page: Page, email: string, password: string) {
  await page.goto('/login');
  await page.getByLabel('邮箱地址').fill(email);
  await page.getByLabel('密码', { exact: true }).fill(password);
  await page.getByRole('button', { name: '登录', exact: true }).click();
  await expect(page).toHaveURL(/\/me$/u);
}

test('@T07 reset request uses real mail and explicit confirmation before new login', async ({ page }) => {
  const password = process.env.T07_BROWSER_PASSWORD;
  if (!password) throw new Error('Private test password is required.');
  const replacement = `${password} revised`;
  await page.goto('/password-reset');
  await page.getByLabel('邮箱地址').fill('browser-reset@example.test');
  await page.getByRole('button', { name: '发送重置邮件', exact: true }).click();
  await expect(page.getByRole('status').filter({ hasText: '请检查邮箱' })).toBeVisible();
  const token = await resetToken('browser-reset@example.test');
  await page.evaluate((secret) => { globalThis.location.hash = `token=${secret}`; }, token);
  await page.reload();
  await expect(page).toHaveURL(/\/password-reset$/u);
  await page.getByLabel('新密码', { exact: true }).fill(replacement);
  await page.getByRole('button', { name: '重置密码', exact: true }).click();
  await expect(page).toHaveURL(/\/login$/u);
  await page.goto('/me');
  await expect(page).toHaveURL(/\/login$/u);
  await login(page, 'browser-reset@example.test', replacement);
});

test('@T07 password change requires real current-password confirmation', async ({ page }) => {
  const password = process.env.T07_BROWSER_PASSWORD;
  if (!password) throw new Error('Private test password is required.');
  await login(page, 'browser-change@example.test', password);
  await page.goto('/me/password/change');
  await page.getByLabel('当前密码', { exact: true }).fill(password);
  await page.getByRole('button', { name: '确认当前密码', exact: true }).click();
  await expect(page.getByRole('status').filter({ hasText: '确认' })).toBeVisible();
  const replacement = `${password} replacement`;
  await page.getByLabel('新密码', { exact: true }).fill(replacement);
  await page.getByRole('button', { name: '修改密码', exact: true }).click();
  await expect(page).toHaveURL(/\/login$/u);
  await login(page, 'browser-change@example.test', replacement);
});
