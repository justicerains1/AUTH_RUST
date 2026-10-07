import { test, expect } from '@playwright/test';
import { createHmac } from 'node:crypto';
function totp(secret: string, seconds: number): string {
  let bits = 0, buffer = 0; const bytes: number[] = [];
  for (const letter of secret) { const digit = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'.indexOf(letter); if (digit < 0) throw new Error('Invalid authenticator secret.'); buffer = (buffer << 5) | digit; bits += 5; if (bits >= 8) { bits -= 8; bytes.push((buffer >> bits) & 255); buffer &= (1 << bits) - 1; } }
  const counter = Buffer.alloc(8); counter.writeBigUInt64BE(BigInt(Math.floor(seconds / 30)));
  const digest = createHmac('sha1', Buffer.from(bytes)).update(counter).digest();
  return String((digest.readUInt32BE((digest[19] ?? 0) & 15) & 0x7fffffff) % 1_000_000).padStart(6, '0');
}
async function clock(advance = false): Promise<number> {
  const key = process.env.T08_CLOCK_KEY; if (!key) throw new Error('Private test clock key required.');
  const response = await fetch(`http://127.0.0.1:5191/__test/clock${advance ? '?advance=1' : ''}`, { headers: { 'x-test-key': key } });
  if (!response.ok) throw new Error('Test clock control failed.');
  const value = await response.json() as { seconds: number }; return value.seconds;
}

test('@T08 real TOTP enrollment and second-factor login, followed by recovery login', async ({ page }) => {
  const password = process.env.T08_BROWSER_PASSWORD; if (!password) throw new Error('Private test password required.');
  await page.goto('/login'); await page.getByLabel('邮箱地址').fill('browser-mfa-enroll@example.test'); await page.getByLabel('密码', { exact: true }).fill(password); await page.getByRole('button', { name: '登录', exact: true }).click(); await expect(page).toHaveURL(/\/me$/u);
  await page.goto('/me/mfa'); await page.getByLabel('当前密码', { exact: true }).fill(password); await page.getByRole('button', { name: '确认当前密码', exact: true }).click();
  await page.getByRole('button', { name: '开始绑定TOTP', exact: true }).click();
  const secret = await page.locator('.setup-secret').textContent(); if (!secret) throw new Error('Enrollment secret missing.');
  await page.getByLabel('验证码', { exact: true }).fill(totp(secret, await clock())); await page.getByRole('button', { name: '确认启用TOTP', exact: true }).click();
  const recovery = await page.getByRole('region', { name: '一次性恢复码' }).locator('li code').first().textContent(); if (!recovery) throw new Error('Recovery code missing.');
  await page.goto('/me'); await page.getByRole('button', { name: '退出当前设备', exact: true }).click(); await page.getByRole('dialog').getByRole('button', { name: '确认退出', exact: true }).click(); await expect(page).toHaveURL(/\/login$/u);
  await page.getByLabel('邮箱地址').fill('browser-mfa-enroll@example.test'); await page.getByLabel('密码', { exact: true }).fill(password); await page.getByRole('button', { name: '登录', exact: true }).click(); await expect(page.getByRole('status').filter({ hasText: '第二因素' })).toBeVisible();
  await page.getByLabel('验证码', { exact: true }).fill(totp(secret, await clock(true))); await page.getByRole('button', { name: '验证并登录', exact: true }).click(); await expect(page).toHaveURL(/\/me$/u);
  await page.getByRole('button', { name: '退出当前设备', exact: true }).click(); await page.getByRole('dialog').getByRole('button', { name: '确认退出', exact: true }).click();
  await page.getByLabel('邮箱地址').fill('browser-mfa-enroll@example.test'); await page.getByLabel('密码', { exact: true }).fill(password); await page.getByRole('button', { name: '登录', exact: true }).click(); await page.getByRole('button', { name: '使用恢复码', exact: true }).click();
  await page.getByLabel('恢复码', { exact: true }).fill(recovery); await page.getByRole('button', { name: '验证并登录', exact: true }).click(); await expect(page).toHaveURL(/\/me$/u);
});
