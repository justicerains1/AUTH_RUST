import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import * as oidc from 'openid-client';
import * as oauth from 'oauth4webapi';
import { createLocalJWKSet, jwtVerify, SignJWT, generateKeyPair, exportJWK } from 'jose';

const issuer = new URL('http://localhost:5190');
const secret = await readFile(process.env.T11_CLIENT_SECRET_FILE, 'utf8');
const password = process.env.T11_BROWSER_PASSWORD;
if (!password) throw new Error('Private interoperability password is required.');
let actualTokenResponse;
const transport = async (input, init) => {
  const url = new URL(typeof input === 'string' ? input : input instanceof URL ? input.href : input.url);
  const tokenEndpoint = url.origin === issuer.origin && url.pathname === '/oauth/token';
  if (url.origin === issuer.origin) url.host = '127.0.0.1:5191';
  const response = await fetch(url, init);
  if (tokenEndpoint && response.status === 200) actualTokenResponse = response.clone();
  return response;
};
const config = await oidc.discovery(issuer, 't11-a', { client_secret: secret, token_endpoint_auth_method: 'client_secret_basic', id_token_signed_response_alg: 'RS256' }, oidc.ClientSecretBasic(secret), { execute: [oidc.allowInsecureRequests], [oidc.customFetch]: transport });
const verifier = oidc.randomPKCECodeVerifier();
const challenge = await oidc.calculatePKCECodeChallenge(verifier);
const state = oidc.randomState(); const nonce = oidc.randomNonce();
let cookie = '';
const csrfResponse = await transport(new URL('/api/v1/auth/csrf', issuer)); cookie = csrfResponse.headers.get('set-cookie').split(';')[0];
let csrf = (await csrfResponse.json()).csrf_token;
const post = (path, body) => transport(new URL(path, issuer), { method: 'POST', redirect: 'manual', headers: { 'Content-Type': 'application/json', Origin: issuer.origin, Cookie: cookie, 'X-CSRF-Token': csrf }, body: JSON.stringify(body) });
const login = await post('/api/v1/auth/login/password', { email: 'interop@example.test', password }); assert.equal(login.status, 200);
cookie = login.headers.getSetCookie().filter((value) => !value.includes('Max-Age=0')).map((value) => value.split(';')[0]).join('; '); const logged = await login.json(); csrf = logged.csrf_token;
const authorization = oidc.buildAuthorizationUrl(config, { redirect_uri: 'http://localhost:5190/callback', scope: 'openid email', state, nonce, code_challenge: challenge, code_challenge_method: 'S256', prompt: 'consent' });
const response = await transport(authorization, { redirect: 'manual', headers: { Cookie: cookie } }); assert.equal(response.status, 302);
const path = response.headers.get('location'); assert.ok(path.startsWith('/oauth/consent/'));
const decision = await post(`/api/v1/oauth/transactions/${path.split('/').at(-1)}/decision`, { decision: 'approve' }); assert.equal(decision.status, 200);
const callback = new URL((await decision.json()).redirect_to);
const tokens = await oidc.authorizationCodeGrant(config, callback, { expectedState: state, expectedNonce: nonce, pkceCodeVerifier: verifier, idTokenExpected: true });
const claims = tokens.claims(); assert.ok(claims && claims.iss === issuer.origin && claims.aud === 't11-a' && claims.nonce === nonce && typeof claims.auth_time === 'number' && Array.isArray(claims.amr) && typeof claims.sid === 'string');
const jwks = await (await transport(new URL('/oauth/jwks', issuer))).json(); const publicKeys = createLocalJWKSet(jwks);
const verified = await jwtVerify(tokens.id_token, publicKeys, { issuer: issuer.origin, audience: 't11-a', algorithms: ['RS256'] }); assert.equal(verified.protectedHeader.alg, 'RS256');
// Reprocess the same real successful exchange response in memory; never retry its consumed code.
assert.ok(actualTokenResponse, 'The actual successful token HTTP response must be captured.');
const actualBody = await actualTokenResponse.clone().json();
assert.ok(actualBody.id_token === tokens.id_token && verified.payload.nonce === nonce, 'The independently verified real ID Token must retain its original nonce.');
const wrongNonce = oidc.randomNonce(); assert.ok(wrongNonce !== nonce, 'The negative expected nonce must differ.');
let nonceRejected = false;
try {
  await oauth.processAuthorizationCodeResponse(config.serverMetadata(), config.clientMetadata(), actualTokenResponse.clone(), { expectedNonce: wrongNonce, requireIdToken: true });
} catch (error) {
  assert.equal(error.code, oauth.JWT_CLAIM_COMPARISON, 'The failure must be a claim comparison, not a signature, time, token or transport error.');
  assert.equal(error.cause?.claim, 'nonce', 'The rejected claim must specifically be nonce.');
  assert.ok(error.cause?.expected === wrongNonce && error.cause?.claims?.nonce === nonce, 'The mature client must compare the incorrect expectation against the unchanged original nonce.');
  nonceRejected = true;
}
assert.ok(nonceRejected, 'The mature client must reject an incorrect expected nonce.');
const correctResponse = actualTokenResponse.clone();
const accepted = await oauth.processAuthorizationCodeResponse(config.serverMetadata(), config.clientMetadata(), correctResponse, { expectedNonce: nonce, requireIdToken: true });
await oauth.validateApplicationLevelSignature(config.serverMetadata(), correctResponse, { [oauth.customFetch]: transport, [oauth.allowInsecureRequests]: true });
assert.ok(oauth.getValidatedIdTokenClaims(accepted)?.nonce === nonce && accepted.id_token === tokens.id_token, 'The same valid token response must succeed with the correct nonce and mature signature validation.');
console.log('PASS T11 E12 mature OIDC processing rejects the wrong expected nonce specifically, then accepts the same real RS256 token response with the correct nonce.');
for (const options of [{ issuer: 'https://wrong.example', audience: 't11-a' }, { issuer: issuer.origin, audience: 'wrong-client' }, { issuer: issuer.origin, audience: 't11-a', currentDate: new Date((Number(verified.payload.exp) + 1) * 1000) }]) await assert.rejects(jwtVerify(tokens.id_token, publicKeys, { ...options, algorithms: ['RS256'] }));
const [header, payload] = tokens.id_token.split('.');
const none = `${Buffer.from(JSON.stringify({ alg: 'none', kid: verified.protectedHeader.kid })).toString('base64url')}.${payload}.`;
await assert.rejects(jwtVerify(none, publicKeys, { issuer: issuer.origin, audience: 't11-a', algorithms: ['RS256'] }));
const hs = await new SignJWT(verified.payload).setProtectedHeader({ alg: 'HS256', kid: verified.protectedHeader.kid }).sign(new Uint8Array(32).fill(7));
await assert.rejects(jwtVerify(hs, publicKeys, { issuer: issuer.origin, audience: 't11-a', algorithms: ['RS256'] }));
const alien = await generateKeyPair('RS256'); const alienKey = await exportJWK(alien.publicKey); alienKey.kid = verified.protectedHeader.kid;
await assert.rejects(jwtVerify(tokens.id_token, createLocalJWKSet({ keys: [alienKey] }), { issuer: issuer.origin, audience: 't11-a', algorithms: ['RS256'] }));
assert.ok(header);
console.log('PASS T11-OIDC-01 mature openid-client discovery/Basic/PKCE/nonce exchange plus independent jose RS256 claims validation.');
console.log('PASS T11 independent JOSE rejects none/HS256, wrong issuer/audience/key and expiry without printing tokens.');
