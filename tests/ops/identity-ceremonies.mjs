// Actual browser WebAuthn and independent TOTP client. No bypass endpoint authenticates users.
import assert from 'node:assert/strict';
import { createHmac } from 'node:crypto';

function totp(secret, seconds) {
  let bits = 0, buffer = 0; const bytes = [];
  for (const letter of secret) { const digit = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'.indexOf(letter); assert.ok(digit >= 0); buffer = (buffer << 5) | digit; bits += 5; if (bits >= 8) { bits -= 8; bytes.push((buffer >> bits) & 255); buffer &= (1 << bits) - 1; } }
  const counter = Buffer.alloc(8); counter.writeBigUInt64BE(BigInt(Math.floor(seconds / 30)));
  const digest = createHmac('sha1', Buffer.from(bytes)).update(counter).digest();
  return String((digest.readUInt32BE((digest[19] ?? 0) & 15) & 0x7fffffff) % 1_000_000).padStart(6, '0');
}
export async function ceremonyClient(context, issuer, controlKey) {
  const page = await context.newPage(); await page.goto(issuer);
  let csrf;
  csrf = (await request('/api/v1/auth/csrf')).body.csrf_token;
  async function request(path, body, method = body === undefined ? 'GET' : 'POST') {
    return page.evaluate(async ({ path, body, method, csrf }) => {
      const response = await fetch(path, { method, credentials: 'same-origin', redirect: 'manual', ...(body === undefined ? {} : { headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify(body) }) });
      let value; try { value = await response.json(); } catch { value = null; }
      return { status: response.status, body: value };
    }, { path, body, method, csrf });
  }
  async function clock() { const response = await fetch(`${issuer}/__restore/clock?advance=1`, { headers: { 'x-test-key': controlKey } }); assert.equal(response.status, 200); return (await response.json()).seconds; }
  async function password(email, password, factor) {
    const response = await request('/api/v1/auth/login/password', { email, password }); assert.equal(response.status, 200);
    if (response.body.csrf_token) csrf = response.body.csrf_token;
    if (response.body.status === 'mfa_required') {
      assert.ok(factor); assert.equal((await request('/api/v1/me')).status, 401);
      csrf = (await request('/api/v1/auth/csrf')).body.csrf_token;
      const result = await request('/api/v1/auth/mfa/totp/verify', { challenge_id: response.body.challenge_id, code: totp(factor, await clock()) }); assert.equal(result.status, 200); csrf = result.body.csrf_token; return result.body;
    }
    assert.equal(response.body.status, 'authenticated'); return response.body;
  }
  async function passkey(kind) {
    const response = await request(kind === 'create' ? '/api/v1/me/passkeys/registration/options' : kind === 'reauth' ? '/api/v1/me/reauth/passkeys/options' : '/api/v1/auth/passkeys/options', {}, 'POST'); assert.equal(response.status, 200);
    const payload = await page.evaluate(async ({ input, kind }) => {
      const decode = (text) => Uint8Array.from(atob(text.replace(/-/gu, '+').replace(/_/gu, '/')), (c) => c.charCodeAt(0));
      const encode = (value) => btoa(String.fromCharCode(...new Uint8Array(value))).replace(/\+/gu, '-').replace(/\//gu, '_').replace(/=+$/gu, '');
      const publicKey = { ...input.publicKey, challenge: decode(input.publicKey.challenge) };
      if (kind === 'create') { publicKey.user = { ...publicKey.user, id: decode(publicKey.user.id) }; publicKey.excludeCredentials = (publicKey.excludeCredentials ?? []).map((item) => ({ ...item, id: decode(item.id) })); }
      else if (publicKey.allowCredentials) publicKey.allowCredentials = publicKey.allowCredentials.map((item) => ({ ...item, id: decode(item.id) }));
      const result = kind === 'create' ? await navigator.credentials.create({ publicKey }) : await navigator.credentials.get({ publicKey });
      const extensions = result.getClientExtensionResults(); const clientExtensionResults = {}; if (extensions.credProps) clientExtensionResults.credProps = extensions.credProps; if (extensions.appid !== undefined) clientExtensionResults.appid = extensions.appid;
      return { challenge_id: input.challenge_id, ...(kind === 'create' ? { name: 'Recovered virtual authenticator' } : {}), credential: { id: result.id, rawId: encode(result.rawId), type: 'public-key', ...(result.authenticatorAttachment ? { authenticatorAttachment: result.authenticatorAttachment } : {}), clientExtensionResults,
        response: kind === 'create' ? { clientDataJSON: encode(result.response.clientDataJSON), attestationObject: encode(result.response.attestationObject), transports: result.response.getTransports() } : { clientDataJSON: encode(result.response.clientDataJSON), authenticatorData: encode(result.response.authenticatorData), signature: encode(result.response.signature), userHandle: result.response.userHandle ? encode(result.response.userHandle) : null } } };
    }, { input: response.body, kind });
    const verified = await request(kind === 'create' ? '/api/v1/me/passkeys/registration/verify' : kind === 'reauth' ? '/api/v1/me/reauth/passkeys/verify' : '/api/v1/auth/passkeys/verify', payload); assert.equal(verified.status, kind === 'create' ? 201 : 200);
    if (verified.body.csrf_token) csrf = verified.body.csrf_token;
    return verified.body;
  }
  return { page, request, password, passkey, clock, enroll: async () => { const enrollment = await request('/api/v1/me/mfa/totp/enrollment', {}); assert.equal(enrollment.status, 200); const factor = enrollment.body.secret; assert.equal((await request('/api/v1/me/mfa/totp/enrollment/confirm', { challenge_id: enrollment.body.challenge_id, code: totp(factor, await clock()) })).status, 200); return factor; }, close: () => page.close() };
}
