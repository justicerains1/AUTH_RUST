import { test, expect, type Page, type Locator } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { randomBytes } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const password = () => { const value = process.env.T23_ACCESSIBILITY_BROWSER_PASSWORD; if (!value) throw new Error('Private test password required.'); return value; };
async function keyTo(page: Page, target: Locator) {
  await expect(target).toBeVisible();
  const history = [];
  for (let count = 0; count < 100; count += 1) {
    const step = await target.evaluate((element) => { const active = document.activeElement; if (active === element) return null; return active?.compareDocumentPosition(element) && (active.compareDocumentPosition(element) & Node.DOCUMENT_POSITION_PRECEDING) !== 0 ? 'Shift+Tab' : 'Tab'; });
    if (step === null) return;
    await page.keyboard.press(step); history.push(await page.evaluate(() => { const active = document.activeElement; return { tag: active?.tagName, label: active?.getAttribute('aria-label') ?? active?.textContent.slice(0, 100) }; }));
  }
  const details = await target.evaluate((element) => ({ tag: element.tagName, label: element.textContent.slice(0, 100), width: innerWidth, heading: document.querySelector('main h1')?.textContent }));
  throw new Error(`Control was not reachable through sequential keyboard navigation: ${JSON.stringify({ target: details, history })}`);
}
async function enter(page: Page, target: Locator) { await keyTo(page, target); await page.keyboard.press('Enter'); }
async function type(page: Page, target: Locator, value: string) { await keyTo(page, target); await page.keyboard.press('Control+A'); await page.keyboard.type(value); }
async function material(browser: string): Promise<{ recoveries: string[] }> {
  const key = process.env.T23_ACCESSIBILITY_CLOCK_KEY; if (!key) throw new Error('Private control key required.');
  if (!['chromium', 'firefox', 'dialogs-chromium', 'dialogs-firefox'].includes(browser)) throw new Error('Fixed browser fixture required.');
  const response = await fetch(`http://127.0.0.1:5322/__test/material?browser=${browser}`, { headers: { 'x-test-key': key } });
  expect(response.status).toBe(200); return response.json() as Promise<{ recoveries: string[] }>;
}
async function login(page: Page, email: string, admin = false) {
  await page.goto('/login'); await expect(page.getByRole('heading', { name: '登录', level: 1, exact: true })).toBeVisible();
  await type(page, page.getByLabel('邮箱地址'), email); await type(page, page.getByLabel('密码', { exact: true }), password()); await enter(page, page.getByRole('button', { name: '登录', exact: true }));
  if (admin) { const value = await material(email.slice('access-admin-'.length, email.indexOf('@'))); const recovery = value.recoveries[0]; if (!recovery) throw new Error('Unused actual recovery code missing.'); await enter(page, page.getByRole('button', { name: '使用恢复码', exact: true })); await type(page, page.getByLabel('恢复码', { exact: true }), recovery); await enter(page, page.getByRole('button', { name: '验证并登录', exact: true })); }
  await expect(page).toHaveURL(/\/me$/u); await expect(page.getByRole('heading', { name: '账号安全', level: 1, exact: true })).toBeVisible();
}
async function mail(email: string, kind: 'verify' | 'reset') {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    const response = await fetch('http://127.0.0.1:8025/api/v1/messages'); const value = await response.json() as { messages: { ID: string; To: { Address: string }[] }[] };
    for (const item of value.messages) { if (!item.To.some((to) => to.Address === email)) continue; const detail = await (await fetch(`http://127.0.0.1:8025/api/v1/message/${item.ID}`)).json() as { Text: string }; if (!detail.Text.includes(kind === 'verify' ? '/email-verification#token=' : '/password-reset#token=')) continue; const token = /#token=([A-Za-z0-9_-]{43})/u.exec(detail.Text)?.[1]; if (token) return token; }
    await new Promise((done) => setTimeout(done, 100));
  }
  throw new Error('Actual account-action email did not arrive.');
}
async function privateMailLink(page: Page, path: string, token: string) { await page.goto(path); await expect(page.locator('main h1')).toBeVisible(); await page.evaluate((value) => { location.hash = `token=${value}`; }, token); await page.reload(); await expect.poll(() => new URL(page.url()).hash).toBe(''); }
async function loaded(page: Page) { await expect(page.locator('main h1')).toBeVisible(); await expect(page.locator('main [aria-busy="true"]')).toHaveCount(0); await expect(page.locator('main').getByRole('status').filter({ hasText: /暂时无法|正在加载|正在查询|正在读取/u })).toHaveCount(0); }
async function semantics(page: Page) {
  const result = await page.evaluate(() => {
    const fields = Array.from(document.querySelectorAll<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>('input:not([type=hidden]),textarea,select')).filter((element) => element.getBoundingClientRect().width > 0);
    return { overflow: document.documentElement.scrollWidth > innerWidth, overflowElements: Array.from(document.querySelectorAll('*')).filter((element) => { const rect = element.getBoundingClientRect(); return rect.right > innerWidth + 1 || rect.left < -1; }).slice(0, 12).map((element) => ({ tag: element.tagName, class: element.className, text: element.textContent.slice(0, 90), right: element.getBoundingClientRect().right })), namelessFields: fields.filter((element) => !element.labels?.length && !element.getAttribute('aria-label') && !element.getAttribute('aria-labelledby')).length, reduced: matchMedia('(prefers-reduced-motion: reduce)').matches, activeAnimations: Array.from(document.querySelectorAll('*')).filter((element) => { const style = getComputedStyle(element); return style.animationName !== 'none' && style.animationDuration.split(',').some((duration) => parseFloat(duration) > 0) || style.transitionDuration.split(',').some((duration) => parseFloat(duration) > 0); }).length };
  });
  expect(result.overflow, `Overflow details: ${JSON.stringify(result.overflowElements)}`).toBe(false); expect(result.namelessFields).toBe(0); expect(result.reduced).toBe(true); expect(result.activeAnimations).toBe(0);
  const axe = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa']).analyze(); expect(axe.violations.map((violation) => ({ id: violation.id, impact: violation.impact }))).toEqual([]);
  return { ...result, axePasses: axe.passes.length, incompleteRules: axe.incomplete.map((rule) => rule.id) };
}
async function report(name: string, browser: string, data: unknown) { const directory = process.env.PRODUCT_ACCESSIBILITY_EVIDENCE; if (!directory) throw new Error('Evidence directory required.'); await mkdir(directory, { recursive: true }); await writeFile(resolve(directory, `${browser}-${name}.json`), `${JSON.stringify({ browser, measuredAt: new Date().toISOString(), data, automatedOnly: true }, null, 2)}\n`); }
async function cancelDialog(page: Page, trigger: Locator, cancelName: string) {
  await enter(page, trigger); const dialog = page.getByRole('dialog'); await expect(dialog).toBeVisible(); const cancel = dialog.getByRole('button', { name: cancelName, exact: true }); await expect(cancel).toBeFocused();
  for (let index = 0; index < 8; index += 1) { await page.keyboard.press('Tab'); expect(await dialog.evaluate((element) => element.contains(document.activeElement))).toBe(true); }
  await page.keyboard.press('Escape'); await expect(dialog).toHaveCount(0); await expect(trigger).toBeFocused();
}

test('keyboard public registration, explicit email confirmation, login and recovery', async ({ page }, info) => {
  const email = `access-${randomBytes(10).toString('hex')}@example.test`;
  await page.goto('/login'); await expect(page.getByRole('heading', { name: '登录', level: 1, exact: true })).toBeVisible(); await enter(page, page.getByRole('button', { name: '登录', exact: true }));
  const emailField = page.getByLabel('邮箱地址'); await expect(emailField).toHaveAttribute('aria-invalid', 'true'); const errorId = await emailField.getAttribute('aria-describedby'); expect(errorId).toBeTruthy(); const error = page.locator(`[id="${errorId ?? ''}"]`); await expect(error).toHaveAttribute('role', 'alert');
  await enter(page, page.getByRole('link', { name: '创建账号', exact: true })); await expect(page.getByRole('heading', { name: '创建你的账号。', exact: true })).toBeVisible();
  await type(page, page.getByLabel('邮箱地址'), email); await type(page, page.getByLabel('密码', { exact: true }), password()); await enter(page, page.getByRole('button', { name: '创建账号', exact: true })); await expect(page.getByRole('status').filter({ hasText: '请检查邮箱' })).toBeVisible();
  await privateMailLink(page, '/email-verification', await mail(email, 'verify')); let confirms = 0; page.on('request', (request) => { if (request.url().endsWith('/auth/email-verification/confirm')) confirms += 1; }); expect(confirms).toBe(0); await enter(page, page.getByRole('button', { name: '确认验证邮箱', exact: true })); await expect(page.getByRole('status').filter({ hasText: '邮箱验证完成' })).toHaveAttribute('aria-live', 'polite'); expect(confirms).toBe(1);
  await enter(page, page.getByRole('link', { name: '前往登录', exact: true })); await login(page, email);
  await cancelDialog(page, page.getByRole('button', { name: '退出当前设备', exact: true }), '取消');
  await page.context().clearCookies(); await page.goto('/password-reset'); await expect(page.getByRole('heading', { name: '找回密码', level: 1, exact: true })).toBeVisible(); await type(page, page.getByLabel('邮箱地址'), email); await enter(page, page.getByRole('button', { name: '发送重置邮件', exact: true })); await expect(page.getByRole('status').filter({ hasText: '请检查邮箱' })).toBeVisible();
  await privateMailLink(page, '/password-reset', await mail(email, 'reset')); await type(page, page.getByLabel('新密码', { exact: true }), password()); await enter(page, page.getByRole('button', { name: '重置密码', exact: true })); await expect(page.getByRole('status').filter({ hasText: '新密码已设置' })).toBeVisible();
  const response = await page.request.get('/api/v1/me'); expect(response.status()).toBe(401); await enter(page, page.getByRole('link', { name: '前往登录', exact: true })); await login(page, email);
  const stored = await page.evaluate(() => ({ local: Object.keys(localStorage), session: Object.keys(sessionStorage) })); expect(stored).toEqual({ local: [], session: [] }); await report('keyboard-public', info.project.name, { realRegistration: true, realMail: true, explicitConfirmation: true, resetDoesNotLogin: true, keyboardOnlyControls: true, alertContract: true, liveStatusContract: true });
});

test('account pages four widths, 200 percent text scaling and reduced motion', async ({ page }, info) => {
  await login(page, `access-account-${info.project.name}@example.test`); const measured = []; const scaled = []; const paths = ['/me', '/me/sessions', '/me/grants', '/me/password/change', '/me/mfa', '/me/passkeys'];
  for (const width of [360, 390, 768, 1440]) { await page.setViewportSize({ width, height: 1000 }); for (const path of paths) { await page.goto(path); await loaded(page); measured.push({ width, path, ...await semantics(page) }); } }
  await page.setViewportSize({ width: 390, height: 844 });
  for (const path of paths) { await page.goto(path); await loaded(page); await page.evaluate(() => { document.documentElement.style.fontSize = '32px'; }); expect(await page.evaluate(() => getComputedStyle(document.documentElement).fontSize)).toBe('32px'); scaled.push({ width: 390, path, ...await semantics(page) }); if (path === '/me') await cancelDialog(page, page.getByRole('button', { name: '退出当前设备', exact: true }), '取消'); }
  await report('account-layout', info.project.name, { measured, scaled, textScalingPercent: 200, actualBrowserZoom: 'not yet measured; text scaling is reported separately', screenReaderSpeech: 'not measured' });
});

test('identity dialog keyboard focus, error announcements and cancellation preserve account', async ({ page }, info) => {
  await login(page, `access-account-${info.project.name}@example.test`); await page.goto('/me/password/change'); await expect(page.getByRole('heading', { name: '修改密码', level: 1, exact: true })).toBeVisible();
  const trigger = page.getByRole('button', { name: '确认当前身份', exact: true }); await enter(page, trigger); const dialog = page.getByRole('dialog', { name: '确认当前身份' }); await expect(dialog.getByRole('button', { name: '取消身份确认', exact: true })).toBeFocused();
  await type(page, dialog.getByLabel('当前密码', { exact: true }), 'Incorrect real identity proof'); await enter(page, dialog.getByRole('button', { name: '确认密码', exact: true })); const alert = dialog.getByRole('alert'); await expect(alert).toBeVisible(); await expect(alert).toHaveAttribute('aria-live', 'assertive'); await expect(dialog.getByLabel('当前密码', { exact: true })).toHaveValue('');
  for (let count = 0; count < 6; count += 1) { await page.keyboard.press('Tab'); expect(await dialog.evaluate((element) => element.contains(document.activeElement))).toBe(true); }
  await page.keyboard.press('Escape'); await expect(dialog).toHaveCount(0); await expect(trigger).toBeFocused(); expect((await page.request.get('/api/v1/me')).status()).toBe(200); await report('identity-dialog', info.project.name, { initialCancel: true, tabTrap: true, escapedToTrigger: true, realInvalidPasswordAlert: true, secretCleared: true, accountPreserved: true });
});

test('administrator pages four widths, keyboard confirmation and declared error states', async ({ page }, info) => {
  await login(page, `access-admin-${info.project.name}@example.test`, true); await page.goto('/admin'); await expect(page.getByRole('heading', { name: '用户管理', exact: true })).toBeVisible(); const measured = []; const scaled = []; const panels = [['用户', '用户管理'], ['应用客户端', '应用客户端'], ['管理员', '管理员成员'], ['安全审计', '安全审计']] as const;
  for (const width of [360, 390, 768, 1440]) { await page.setViewportSize({ width, height: 1000 }); for (const [name, heading] of panels) { await enter(page, page.getByRole('button', { name, exact: true })); await expect(page.getByRole('heading', { name: heading, exact: true })).toBeVisible(); await loaded(page); measured.push({ width, panel: name, ...await semantics(page) }); } }
  await enter(page, page.getByRole('button', { name: '应用客户端', exact: true })); await expect(page.getByRole('heading', { name: '应用客户端', exact: true })).toBeVisible(); await enter(page, page.getByRole('button', { name: '创建客户端', exact: true })); await expect(page.getByRole('alert')).toHaveAttribute('aria-live', 'assertive');
  await page.setViewportSize({ width: 390, height: 844 }); await page.evaluate(() => { document.documentElement.style.fontSize = '32px'; }); expect(await page.evaluate(() => getComputedStyle(document.documentElement).fontSize)).toBe('32px');
  for (const [name, heading] of panels) { await enter(page, page.getByRole('button', { name, exact: true })); await expect(page.getByRole('heading', { name: heading, exact: true })).toBeVisible(); await loaded(page); scaled.push({ width: 390, panel: name, ...await semantics(page) }); }
  await report('admin-layout', info.project.name, { measured, scaled, textScalingPercent: 200, realAdminRecoveryLogin: true, keyboardNavigation: true, clientValidationLiveAlert: true });
});

test('administrator high-risk confirmation keyboard trap and cancel never mutates', async ({ page }, info) => {
  await login(page, `access-admin-${info.project.name}@example.test`, true); await page.goto('/admin'); await expect(page.getByRole('heading', { name: '用户管理', exact: true })).toBeVisible();
  await enter(page, page.getByRole('button', { name: '应用客户端', exact: true })); await expect(page.getByRole('heading', { name: '应用客户端', exact: true })).toBeVisible();
  let mutations = 0; page.on('request', (request) => { if (request.method() !== 'GET') mutations += 1; });
  await cancelDialog(page, page.getByRole('row').filter({ hasText: 't23_accessibility-transaction' }).getByRole('button', { name: '停用客户端', exact: true }), '取消'); expect(mutations).toBe(0); await report('admin-dialog', info.project.name, { initialCancel: true, keyboardTrap: true, escapeReturnFocus: true, requestsFromCancel: 0 });
});

test('client secret dialogs and strong authentication keyboard sequence', async ({ page }, info) => {
  await login(page, `access-admin-dialogs-${info.project.name}@example.test`, true); await page.goto('/admin'); await loaded(page); await enter(page, page.getByRole('button', { name: '应用客户端', exact: true })); await loaded(page);
  const name = `Keyboard client ${info.project.name}`; await type(page, page.getByLabel('应用名称', { exact: true }), name); await type(page, page.getByLabel('登录回调地址（每行一个）', { exact: true }), 'http://localhost:5320/callback');
  let creates = 0; let rotations = 0; page.on('request', (request) => { const path = new URL(request.url()).pathname; if (request.method() === 'POST' && path === '/api/v1/admin/clients') creates += 1; if (request.method() === 'POST' && path.endsWith('/rotate-secret')) rotations += 1; });
  async function proof() {
    const dialog = page.getByRole('dialog', { name: '确认当前身份', exact: true }); await expect(dialog.getByRole('button', { name: '取消身份确认', exact: true })).toBeFocused();
    await type(page, dialog.getByLabel('当前密码', { exact: true }), password()); await enter(page, dialog.getByRole('button', { name: '确认密码', exact: true })); await expect(dialog.getByLabel('验证码', { exact: true })).toBeVisible();
    await enter(page, dialog.getByRole('button', { name: '使用恢复码', exact: true })); const value = await material(`dialogs-${info.project.name}`); const recovery = value.recoveries[0]; if (!recovery) throw new Error('Unused real recovery proof missing.'); await type(page, dialog.getByLabel('恢复码', { exact: true }), recovery); await enter(page, dialog.getByRole('button', { name: '完成强认证', exact: true })); await expect(dialog).toHaveCount(0);
  }
  const create = page.getByRole('button', { name: '创建客户端', exact: true }); await enter(page, create); expect(creates).toBe(0); await proof(); await expect(create).toBeFocused(); expect(creates).toBe(0); const createdResponse = page.waitForResponse((response) => response.request().method() === 'POST' && new URL(response.url()).pathname === '/api/v1/admin/clients'); await enter(page, create); const createdStatus = (await createdResponse).status(); expect(createdStatus, 'Actual create status; no private payload is read').toBe(201);
  const secret = page.getByRole('dialog', { name: '保存客户端秘密', exact: true }); const close = secret.getByRole('button', { name: '已保存，关闭秘密', exact: true }); await expect(close).toBeFocused(); expect(creates).toBe(1);
  const createdSecret = await secret.locator('.setup-secret').textContent(); if (!createdSecret) throw new Error('Real generated client secret missing.'); for (let index = 0; index < 4; index += 1) { await page.keyboard.press('Tab'); expect(await secret.evaluate((element) => element.contains(document.activeElement))).toBe(true); } await page.keyboard.press('Escape'); await expect(secret).toHaveCount(0); await expect(create).toBeFocused(); expect(await page.locator('main').innerText()).not.toContain(createdSecret);
  const row = page.getByRole('row').filter({ hasText: name }); const rotate = row.getByRole('button', { name: '轮换客户端秘密', exact: true }); await enter(page, rotate); const action = page.getByRole('dialog', { name: '轮换客户端秘密？', exact: true }); await expect(action.getByRole('button', { name: '取消', exact: true })).toBeFocused();
  await enter(page, action.getByRole('button', { name: '确认身份后继续', exact: true })); expect(rotations).toBe(0); await proof(); await expect(action.getByRole('button', { name: '取消', exact: true })).toBeFocused(); expect(rotations).toBe(0); const rotatedResponse = page.waitForResponse((response) => response.request().method() === 'POST' && new URL(response.url()).pathname.endsWith('/rotate-secret')); await enter(page, action.getByRole('button', { name: '确认轮换客户端秘密', exact: true })); expect((await rotatedResponse).status(), 'Actual rotation status; no private payload is read').toBe(200);
  await expect(close).toBeFocused(); const rotatedSecret = await secret.locator('.setup-secret').textContent(); if (!rotatedSecret) throw new Error('Real rotated secret missing.'); expect(rotatedSecret === createdSecret).toBe(false); expect(rotations).toBe(1); await enter(page, close); await expect(secret).toHaveCount(0); await expect(rotate).toBeFocused(); expect(await page.locator('main').innerText()).not.toContain(rotatedSecret);
  expect(await page.evaluate(() => ({ local: Object.keys(localStorage), session: Object.keys(sessionStorage) }))).toEqual({ local: [], session: [] }); await report('client-dialog-sequence', info.project.name, { keyboardOnly: true, actualRecoveryProofs: 3, createAfterExplicitSecondSubmit: true, createFocusReturned: true, secretInitialFocus: true, secretTabTrap: true, secretEscapeReturned: true, actionReauthenticationActionFocus: true, rotationOnlyAfterExplicitConfirmation: true, rotatedSecretCloseReturned: true, secretRemovedFromDomAndStorage: true });
});
