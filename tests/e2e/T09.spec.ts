import { test, expect, type Page, type CDPSession } from '@playwright/test';
import { createPrivateKey, createHash, sign } from 'node:crypto';

async function authenticator(page: Page): Promise<{ cdp: CDPSession; id: string }> {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('WebAuthn.enable');
  const result = await cdp.send('WebAuthn.addVirtualAuthenticator', { options: { protocol: 'ctap2', transport: 'internal', hasResidentKey: true, hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true } });
  return { cdp, id: result.authenticatorId };
}
async function passwordLogin(page: Page, email: string) {
  const password = process.env.T09_BROWSER_PASSWORD; if (!password) throw new Error('Private test password required.');
  await page.goto('/login'); await page.getByLabel('邮箱地址').fill(email); await page.getByLabel('密码', { exact: true }).fill(password); await page.getByRole('button', { name: '登录', exact: true }).click(); await expect(page).toHaveURL(/\/me$/u);
}
async function registration(page: Page) {
  const password = process.env.T09_BROWSER_PASSWORD; if (!password) throw new Error('Private test password required.');
  await page.goto('/me/passkeys');
  await page.getByLabel('当前密码', { exact: true }).fill(password); await page.getByRole('button', { name: '确认当前密码', exact: true }).click();
  await page.getByLabel('Passkey名称', { exact: true }).fill('Browser authenticator fixture');
  const submitted = page.waitForRequest((request) => request.url().endsWith('/me/passkeys/registration/verify'));
  await page.getByRole('button', { name: '注册Passkey', exact: true }).click();
  const sent = await submitted;
  const shape = sent.postDataJSON() as { credential: { id: string; rawId: string; response: Record<string, unknown>; clientExtensionResults: Record<string, unknown> } };
  console.log('T09 safe registration shape', JSON.stringify({ sameId: shape.credential.id === shape.credential.rawId, responseFields: Object.keys(shape.credential.response), extensionFields: Object.keys(shape.credential.clientExtensionResults) }));
  await expect(page.getByText('Browser authenticator fixture', { exact: true })).toBeVisible();
}
async function logout(page: Page) { await page.goto('/me'); await page.getByRole('button', { name: '退出当前设备', exact: true }).click(); await page.getByRole('dialog').getByRole('button', { name: '确认退出', exact: true }).click(); await expect(page).toHaveURL(/\/login$/u); }

test('@T09 virtual CTAP2 authenticator registers and discovers a strong identity session', async ({ page }) => {
  const { cdp, id } = await authenticator(page);
  try {
    await passwordLogin(page, 'browser-passkey@example.test'); await registration(page); await logout(page);
    const optionsResponse = page.waitForResponse((response) => response.url().endsWith('/auth/passkeys/options'));
    await page.getByRole('button', { name: '使用Passkey登录', exact: true }).click();
    const options = await (await optionsResponse).json() as { publicKey?: Record<string, unknown> };
    console.log('T09 safe assertion-option shape', JSON.stringify(Object.fromEntries(Object.entries(options.publicKey ?? {}).map(([key, value]) => [key, value === null ? 'null' : Array.isArray(value) ? 'array' : typeof value]))));
    await expect(page).toHaveURL(/\/me$/u);
    await expect(page.getByText('browser-passkey@example.test', { exact: true }).first()).toBeVisible();
    await page.goto('/me/mfa'); await page.getByRole('button', { name: '使用Passkey确认身份', exact: true }).click(); await expect(page.getByRole('status').filter({ hasText: '近期认证已完成' })).toBeVisible();
    const password = process.env.T09_BROWSER_PASSWORD; if (!password) throw new Error('Private test password required.');
    await page.goto('/me/password/change');
    await page.getByRole('button', { name: '使用Passkey确认身份', exact: true }).click();
    await page.getByLabel('新密码', { exact: true }).fill(`${password} updated`);
    await page.getByRole('button', { name: '修改密码', exact: true }).click();
    await expect(page).toHaveURL(/\/login$/u);
    const stored = await page.evaluate(() => ({ local: Object.keys(localStorage), session: Object.keys(sessionStorage) })); expect(stored).toEqual({ local: [], session: [] });
  } finally { await cdp.send('WebAuthn.removeVirtualAuthenticator', { authenticatorId: id }); await cdp.detach(); }
});

test('@T09 signed assertion is single-use and user verification cannot be bypassed', async ({ page }) => {
  const { cdp, id } = await authenticator(page);
  try {
    await passwordLogin(page, 'browser-passkey-negative@example.test'); await registration(page); await logout(page);
    let captured: unknown;
    await page.route('**/api/v1/auth/passkeys/verify', async (route) => { captured = route.request().postDataJSON() as unknown; await route.abort(); });
    await page.getByRole('button', { name: '使用Passkey登录', exact: true }).click();
    await expect(page.getByRole('alert').or(page.getByRole('status')).filter({ hasText: '登录未完成' })).toBeVisible();
    await page.unroute('**/api/v1/auth/passkeys/verify');
    if (!captured) throw new Error('A real signed assertion was not captured.');
    const statuses = await page.evaluate(async (proof) => {
      type Signed = { credential: { response: { clientDataJSON: string; signature: string } } };
      const original = proof as Signed;
      const decode = (value: string) => atob(value.replace(/-/gu, '+').replace(/_/gu, '/'));
      const encode = (value: string) => btoa(value).replace(/\+/gu, '-').replace(/\//gu, '_').replace(/=+$/gu, '');
      const csrf = await (await fetch('/api/v1/auth/csrf', { credentials: 'same-origin' })).json() as { csrf_token: string };
      const send = async (body: unknown) => (await fetch('/api/v1/auth/passkeys/verify', { method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf.csrf_token }, body: JSON.stringify(body) })).status;
      const badOrigin = structuredClone(original); const originData = JSON.parse(decode(badOrigin.credential.response.clientDataJSON)) as Record<string, unknown>; originData.origin = 'https://attacker.example'; badOrigin.credential.response.clientDataJSON = encode(JSON.stringify(originData));
      const badChallenge = structuredClone(original); const challengeData = JSON.parse(decode(badChallenge.credential.response.clientDataJSON)) as Record<string, unknown>; challengeData.challenge = 'invalid-challenge'; badChallenge.credential.response.clientDataJSON = encode(JSON.stringify(challengeData));
      const badSignature = structuredClone(original); const signature = decode(badSignature.credential.response.signature); badSignature.credential.response.signature = encode(String.fromCharCode(signature.charCodeAt(0) ^ 1) + signature.slice(1));
      return [await send(badOrigin), await send(badChallenge), await send(badSignature), await send(original)];
    }, captured);
    expect(statuses.slice(0, 3).every((status) => status === 401 || status === 403)).toBe(true);
    expect(statuses[3]).toBe(200);
    await page.goto('/me'); await expect(page).toHaveURL(/\/me$/u);
    const replay = await page.evaluate(async (body) => {
      const csrf = await (await fetch('/api/v1/auth/csrf', { credentials: 'same-origin' })).json() as { csrf_token: string };
      return (await fetch('/api/v1/auth/passkeys/verify', { method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf.csrf_token }, body: JSON.stringify(body) })).status;
    }, captured);
    expect(replay).not.toBe(200);
    await logout(page);
    captured = undefined;
    await page.route('**/api/v1/auth/passkeys/verify', async (route) => { captured = route.request().postDataJSON() as unknown; await route.abort(); });
    await page.getByRole('button', { name: '使用Passkey登录', exact: true }).click();
    await expect(page.getByRole('status').or(page.getByRole('alert')).filter({ hasText: '登录未完成' })).toBeVisible();
    await page.unroute('**/api/v1/auth/passkeys/verify');
    if (!captured) throw new Error('Fresh virtual-authenticator assertion was not captured.');
    type Proof = { credential: { id: string; response: { authenticatorData: string; clientDataJSON: string; signature: string } } };
    const fresh = captured as Proof;
    const exported = await cdp.send('WebAuthn.getCredentials', { authenticatorId: id });
    const credential = exported.credentials.find((item) => Buffer.from(item.credentialId, 'base64').toString('base64url') === fresh.credential.id);
    if (!credential) throw new Error('Virtual-authenticator signing key is unavailable.');
    const changed = structuredClone(fresh);
    const authData = Buffer.from(changed.credential.response.authenticatorData, 'base64url');
    authData[32] = (authData[32] ?? 0) & ~4;
    const clientHash = createHash('sha256').update(Buffer.from(changed.credential.response.clientDataJSON, 'base64url')).digest();
    const key = createPrivateKey({ key: Buffer.from(credential.privateKey, 'base64'), format: 'der', type: 'pkcs8' });
    changed.credential.response.authenticatorData = authData.toString('base64url');
    changed.credential.response.signature = sign('sha256', Buffer.concat([authData, clientHash]), key).toString('base64url');
    expect((authData[32] & 4) === 0).toBe(true);
    const noUvStatus = await page.evaluate(async (proof) => {
      const csrf = await (await fetch('/api/v1/auth/csrf', { credentials: 'same-origin' })).json() as { csrf_token: string };
      const send = async (body: unknown) => (await fetch('/api/v1/auth/passkeys/verify', { method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf.csrf_token }, body: JSON.stringify(body) })).status;
      return [await send(proof.changed), await send(proof.original)];
    }, { changed, original: fresh });
    expect([401, 403]).toContain(noUvStatus[0]);
    expect(noUvStatus[1]).toBe(200);
  } finally { await cdp.send('WebAuthn.removeVirtualAuthenticator', { authenticatorId: id }); await cdp.detach(); }
});
