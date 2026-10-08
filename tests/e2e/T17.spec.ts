import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { randomBytes, createHmac } from 'node:crypto';
import { resolve } from 'node:path';
import { mkdir } from 'node:fs/promises';
async function material(query = ''): Promise<{ secret: string; recovery: string; seconds: number }> { const key = process.env.T17_CLOCK_KEY; if (!key) throw new Error('Private test key required.'); return (await fetch(`http://127.0.0.1:5197/__test/material${query}`, { headers: { 'x-test-key': key } })).json() as Promise<{ secret: string; recovery: string; seconds: number }>; }
function code(secret: string, seconds: number): string { const bytes: number[] = []; let buffer = 0, bits = 0; for (const c of secret) { const value = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'.indexOf(c); buffer = buffer << 5 | value; bits += 5; if (bits >= 8) { bits -= 8; bytes.push(buffer >> bits & 255); buffer &= (1 << bits) - 1; } } const counter = Buffer.alloc(8); counter.writeBigUInt64BE(BigInt(Math.floor(seconds / 30))); const mac = createHmac('sha1', Buffer.from(bytes)).update(counter).digest(); return String((mac.readUInt32BE((mac[19] ?? 0) & 15) & 0x7fffffff) % 1_000_000).padStart(6, '0'); }
async function mail(email: string): Promise<string> { for (let attempt = 0; attempt < 100; attempt += 1) { const list = await (await fetch('http://127.0.0.1:8025/api/v1/messages')).json() as { messages: { ID: string; To: { Address: string }[] }[] }; for (const item of list.messages) { if (!item.To.some((to) => to.Address === email)) continue; const detail = await (await fetch(`http://127.0.0.1:8025/api/v1/message/${item.ID}`)).json() as { Text: string }; const token = /#token=([A-Za-z0-9_-]{43})/u.exec(detail.Text)?.[1]; if (token) return token; } await new Promise((done) => setTimeout(done, 100)); } throw new Error('Actual mail delivery missing.'); }

test('@T17 four-width public pages have accessible keyboard layouts and reduced motion', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' }); await mkdir(resolve('docs/evidence/T17/screenshots'), { recursive: true });
  for (const width of [360, 390, 768, 1440]) { await page.setViewportSize({ width, height: 1000 }); for (const path of ['/', '/login', '/register', '/password-reset', '/email-verification']) { await page.goto(path); await expect(page.locator('main h1')).toBeVisible(); const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth); expect(overflow).toBe(false); const axe = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa']).analyze(); expect(axe.violations.map((violation) => violation.id)).toEqual([]); } await page.goto('/'); await expect(page.locator('main h1')).toBeVisible(); await page.screenshot({ path: resolve(`docs/evidence/T17/screenshots/home-${String(width)}.png`), fullPage: true }); }
  await page.goto('/login'); await page.getByRole('link', { name: '跳到主要内容' }).focus(); await page.keyboard.press('Enter'); await expect(page.locator('#main-content')).toBeFocused();
});

test('@T17 new account registration uses real mail and explicit confirmation before login', async ({ page }) => {
  const email = `public-${randomBytes(12).toString('hex')}@example.test`; const password = process.env.T17_BROWSER_PASSWORD; if (!password) throw new Error('Private test password required.');
  await page.goto('/'); await page.getByRole('link', { name: /登录账号/u }).click(); await page.getByRole('link', { name: '创建账号', exact: true }).click(); await expect(page.getByRole('heading', { name: '创建你的账号。', exact: true })).toBeVisible(); await page.getByLabel('邮箱地址').fill(email); await page.getByLabel('密码', { exact: true }).fill(password); await expect(page.getByLabel('邮箱地址')).toHaveValue(email); await expect(page.getByLabel('密码', { exact: true })).toHaveValue(password); await page.getByRole('button', { name: '创建账号', exact: true }).click(); await expect(page.getByRole('status').filter({ hasText: '请检查邮箱' })).toBeVisible();
  const token = await mail(email); await page.goto('/email-verification'); await page.evaluate((secret) => { globalThis.location.hash = `token=${secret}`; }, token); await page.reload(); await expect(page).toHaveURL(/\/email-verification$/u); await page.getByRole('button', { name: '确认验证邮箱', exact: true }).click(); await expect(page.getByRole('status').filter({ hasText: '邮箱验证完成' })).toBeVisible();
  await page.goto('/login'); await page.getByLabel('邮箱地址').fill(email); await page.getByLabel('密码', { exact: true }).fill(password); await page.getByRole('button', { name: '登录', exact: true }).click(); await expect(page).toHaveURL(/\/me$/u);
});

test('@T17 MFA supports pasted code, recovery switch and actual second-factor login', async ({ page }) => {
  const password = process.env.T17_BROWSER_PASSWORD; if (!password) throw new Error('Private test password required.'); await page.goto('/login'); await page.getByLabel('邮箱地址').fill('browser-public@example.test'); await page.getByLabel('密码', { exact: true }).fill(password); await page.getByRole('button', { name: '登录', exact: true }).click(); await expect(page.getByRole('status').filter({ hasText: '第二因素' })).toBeVisible();
  await page.getByRole('button', { name: '使用恢复码', exact: true }).click(); await page.getByLabel('恢复码', { exact: true }).fill((await material()).recovery); await page.getByRole('button', { name: '使用验证码', exact: true }).click(); await expect(page.getByLabel('验证码', { exact: true })).toHaveValue('');
  const value = await material('?advance=1'); await page.getByLabel('验证码', { exact: true }).fill(code(value.secret, value.seconds)); await page.getByRole('button', { name: '验证并登录', exact: true }).click(); await expect(page).toHaveURL(/\/me$/u);
  const stored = await page.evaluate(() => ({ local: Object.keys(localStorage), session: Object.keys(sessionStorage) })); expect(stored).toEqual({ local: [], session: [] });
});

test('@T17 real rate limit counts down without automatic password retry', async ({ page }) => {
  const email = `limited-${randomBytes(12).toString('hex')}@example.test`; await page.goto('/login'); let submissions = 0; page.on('request', (request) => { if (request.url().endsWith('/auth/login/password')) submissions += 1; });
  for (let attempt = 0; attempt < 6; attempt += 1) { await page.getByLabel('邮箱地址').fill(email); await page.getByLabel('密码', { exact: true }).fill('Wrong fixture password phrase 17409'); await page.getByRole('button', { name: '登录', exact: true }).click(); await expect(page.getByLabel('密码', { exact: true })).toHaveValue(''); }
  await expect(page.getByRole('button', { name: '登录', exact: true })).toBeDisabled(); const count = submissions; await page.waitForTimeout(1200); expect(submissions).toBe(count); await expect(page.getByLabel('邮箱地址')).toHaveValue(email);
});

test('@T17 expired MFA challenge restarts and cancellation falls back to password', async ({ page }) => {
  const password = process.env.T17_BROWSER_PASSWORD; if (!password) throw new Error('Private test password required.');
  await page.goto('/login'); await page.getByLabel('邮箱地址').fill('browser-public@example.test'); await page.getByLabel('密码', { exact: true }).fill(password); await page.getByRole('button', { name: '登录', exact: true }).click(); await expect(page.getByRole('status').filter({ hasText: '第二因素' })).toBeVisible();
  const value = await material('?expire=1'); await page.getByLabel('验证码', { exact: true }).fill(code(value.secret, value.seconds)); await page.getByRole('button', { name: '验证并登录', exact: true }).click(); await expect(page.getByRole('alert').filter({ hasText: '验证已过期' })).toBeVisible(); await page.getByRole('button', { name: '重新开始登录', exact: true }).click(); await expect(page.getByLabel('密码', { exact: true })).toBeVisible();
  const cdp = await page.context().newCDPSession(page); await cdp.send('WebAuthn.enable'); const auth = await cdp.send('WebAuthn.addVirtualAuthenticator', { options: { protocol: 'ctap2', transport: 'internal', hasResidentKey: true, hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: false } });
  await page.evaluate(() => {
    const original = navigator.credentials.get.bind(navigator.credentials);
    navigator.credentials.get = (options) => { const controller = new AbortController(); setTimeout(() => { controller.abort(); }, 1000); return original({ ...options, signal: controller.signal }); };
  });
  try { await page.getByRole('button', { name: '使用通行密钥登录', exact: true }).click(); await expect(page.getByRole('alert').filter({ hasText: '通行密钥验证已取消' })).toBeVisible(); await page.getByRole('button', { name: '改用密码登录', exact: true }).click(); await expect(page.getByLabel('密码', { exact: true })).toBeFocused(); }
  finally { await cdp.send('WebAuthn.removeVirtualAuthenticator', { authenticatorId: auth.authenticatorId }); await cdp.detach(); }
});

test('@T17 validated OAuth transaction survives public recovery navigation', async ({ page }) => {
  const query = new URLSearchParams({ client_id: 't17-transaction', response_type: 'code', redirect_uri: 'http://localhost:5190/callback', scope: 'openid', state: 'T17_BROWSER_STATE_12345', nonce: 'T17_BROWSER_NONCE_12345', code_challenge: 'A'.repeat(43), code_challenge_method: 'S256' });
  await page.goto(`/oauth/authorize?${query}`); await expect(page).toHaveURL(/\/login\?transaction=[0-9a-f-]{36}$/u);
  const transaction = new URL(page.url()).searchParams.get('transaction'); if (!transaction) throw new Error('Verified transaction was not created.');
  await page.getByRole('link', { name: '创建账号', exact: true }).click(); expect(new URL(page.url()).searchParams.get('transaction') === transaction).toBe(true);
  await page.getByRole('link', { name: '前往登录', exact: true }).click(); expect(new URL(page.url()).searchParams.get('transaction') === transaction).toBe(true);
  await page.getByRole('link', { name: '忘记密码？', exact: true }).click(); expect(new URL(page.url()).searchParams.get('transaction') === transaction).toBe(true);
});
