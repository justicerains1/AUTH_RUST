import { test, expect, type Page } from '@playwright/test';
import { randomBytes } from 'node:crypto';

async function verificationToken(email: string, previous?: string): Promise<string> {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    const response = await fetch('http://127.0.0.1:8025/api/v1/messages');
    const data = await response.json() as { messages: { ID: string; To: { Address: string }[] }[] };
    for (const mail of data.messages) {
      if (!mail.To.some((to) => to.Address === email)) continue;
      const message = await (await fetch(`http://127.0.0.1:8025/api/v1/message/${mail.ID}`)).json() as { Text: string };
      const token = /#token=([A-Za-z0-9_-]{43})/u.exec(message.Text)?.[1];
      if (token && token !== previous) return token;
    }
    await new Promise((resolveDelay) => setTimeout(resolveDelay, 100));
  }
  throw new Error('Verification mail was not delivered.');
}
async function openPrivateLink(page: Page, token: string) {
  // Avoid token-bearing goto errors/trace URLs; set hash inside the page then reload.
  await page.goto('/email-verification');
  await page.evaluate((value) => { window.location.hash = `token=${value}`; }, token);
  await page.reload();
  await expect(page).toHaveURL(/\/email-verification$/u);
}

test('@T05 real registration and explicit email confirmation', async ({ page }) => {
  const email = `e2e-${randomBytes(12).toString('hex')}@example.test`;
  await page.goto('/register');
  await page.getByLabel('邮箱地址').fill(email);
  await page.getByLabel('密码', { exact: true }).fill('Browser fixture unique passphrase 89431');
  await page.getByRole('button', { name: '创建账号', exact: true }).click();
  await expect(page.getByRole('status').filter({ hasText: '请检查邮箱' })).toBeVisible();
  await expect(page.getByLabel('密码', { exact: true })).toHaveValue('');
  const token = await verificationToken(email);
  let confirms = 0;
  page.on('request', (request) => { if (request.url().endsWith('/auth/email-verification/confirm')) confirms += 1; });
  await openPrivateLink(page, token);
  await expect(page.getByRole('button', { name: '确认验证邮箱', exact: true })).toBeVisible();
  expect(confirms).toBe(0);
  await page.getByRole('button', { name: '确认验证邮箱', exact: true }).click();
  await expect(page.getByRole('status').filter({ hasText: '邮箱验证完成' })).toBeVisible();
  expect(confirms).toBe(1);
  const storage = await page.evaluate(() => ({ local: Object.keys(localStorage), session: Object.keys(sessionStorage) }));
  expect(storage).toEqual({ local: [], session: [] });
});

test('@T05 resend invalidates the old link and verifies the new link', async ({ page }) => {
  const email = `e2e-resend-${randomBytes(12).toString('hex')}@example.test`;
  await page.goto('/register');
  await page.getByLabel('邮箱地址').fill(email);
  await page.getByLabel('密码', { exact: true }).fill('Browser fixture unique passphrase 89431');
  await page.getByRole('button', { name: '创建账号', exact: true }).click();
  await expect(page.getByRole('status').filter({ hasText: '请检查邮箱' })).toBeVisible();
  const old = await verificationToken(email);
  await page.goto('/email-verification');
  await page.getByLabel('邮箱地址').fill(email);
  await page.getByRole('button', { name: '重新发送验证邮件', exact: true }).click();
  await expect(page.getByRole('status').filter({ hasText: '请检查邮箱' })).toBeVisible();
  const current = await verificationToken(email, old);
  await openPrivateLink(page, old);
  await page.getByRole('button', { name: '确认验证邮箱', exact: true }).click();
  await expect(page.getByRole('alert').filter({ hasText: '验证暂未完成' })).toBeVisible();
  await openPrivateLink(page, current);
  await page.getByRole('button', { name: '确认验证邮箱', exact: true }).click();
  await expect(page.getByRole('status').filter({ hasText: '邮箱验证完成' })).toBeVisible();
});
