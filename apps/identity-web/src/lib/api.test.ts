import { describe, expect, it, vi } from 'vitest';
import { z } from 'zod';
import { ApiClient, ApiError } from './api';

const token = 'TEST_INVALID_CSRF_'.padEnd(43, 'a');
const schema = z.object({ ok: z.boolean() });
const json = (value: unknown, status = 200, headers: HeadersInit = {}) => {
  const merged = new Headers(headers); merged.set('Content-Type', 'application/json');
  return new Response(JSON.stringify(value), { status, headers: merged });
};
const inputPath = (input: RequestInfo | URL) => typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
const failure = (code: string) => ({ error: { code, message: '请求失败', request_id: 'c09d1bb5-0018-4f9b-8d39-3f48e22f3007' } });

describe('API transport and CSRF boundaries', () => {
  it('invokes browser transport with the global receiver required by native fetch', async () => {
    let usedGlobalReceiver = false;
    function transport(this: unknown): Promise<Response> {
      usedGlobalReceiver = this === globalThis;
      if (this !== globalThis) return Promise.reject(new TypeError('Illegal invocation'));
      return Promise.resolve(json({ ok: true }));
    }
    await expect(new ApiClient(transport).request('/me', schema)).resolves.toEqual({ ok: true });
    expect(usedGlobalReceiver).toBe(true);
  });

  it('shares preauthentication CSRF fetch and sends cookies without persisting secrets', async () => {
    const transport = vi.fn<typeof fetch>().mockImplementation((input) => Promise.resolve(inputPath(input).endsWith('/csrf') ? json({ csrf_token: token }) : json({ ok: true })));
    const client = new ApiClient(transport);
    await Promise.all([client.request('/auth/register', schema, { method: 'POST', body: { email: 'user@example.test' } }), client.request('/auth/register', schema, { method: 'POST', body: {} })]);
    expect(transport.mock.calls.filter(([input]) => inputPath(input).endsWith('/csrf'))).toHaveLength(1);
    for (const [input, options] of transport.mock.calls) {
      expect(options?.credentials).toBe('same-origin');
      if (!inputPath(input).endsWith('/csrf')) expect(new Headers(options?.headers).get('X-CSRF-Token')).toBe(token);
    }
  });

  it('invalidates rejected CSRF and never silently replays the failed mutation', async () => {
    let mutation = 0;
    const transport = vi.fn<typeof fetch>().mockImplementation((input) => {
      if (inputPath(input).endsWith('/csrf')) return Promise.resolve(json({ csrf_token: token }));
      mutation += 1;
      return Promise.resolve(mutation === 1 ? json(failure('AUTH_CSRF_INVALID'), 403) : json({ ok: true }));
    });
    const client = new ApiClient(transport);
    await expect(client.request('/me/password/change', schema, { method: 'POST', body: {} })).rejects.toMatchObject({ code: 'AUTH_CSRF_INVALID', status: 403 });
    expect(mutation).toBe(1);
    await client.request('/me/password/change', schema, { method: 'POST', body: {} });
    expect(transport.mock.calls.filter(([input]) => inputPath(input).endsWith('/csrf'))).toHaveLength(2);
  });

  it('accepts login rotation and clears old CSRF after an expired session', async () => {
    const transport = vi.fn<typeof fetch>().mockResolvedValueOnce(json(failure('AUTH_SESSION_REQUIRED'), 401)).mockResolvedValueOnce(json({ csrf_token: token })).mockResolvedValueOnce(json({ ok: true }));
    const client = new ApiClient(transport);
    client.acceptRotatedCsrf('OLD_TEST_INVALID_CSRF_'.padEnd(43, 'b'));
    await expect(client.request('/me', schema)).rejects.toBeInstanceOf(ApiError);
    await client.request('/auth/logout', schema, { method: 'POST' });
    expect(new Headers(transport.mock.calls[2]?.[1]?.headers).get('X-CSRF-Token')).toBe(token);
  });

  it('returns retry information and rejects malformed responses', async () => {
    const transport = vi.fn<typeof fetch>().mockResolvedValueOnce(json(failure('RATE_LIMITED'), 429, { 'Retry-After': '60' })).mockResolvedValueOnce(json({ unexpected: 'field' }));
    const client = new ApiClient(transport);
    await expect(client.request('/me', schema)).rejects.toMatchObject({ code: 'RATE_LIMITED', retryAfter: 60 });
    await expect(client.request('/me', schema)).rejects.toMatchObject({ code: 'CLIENT_INVALID_RESPONSE' });
  });

  it('preserves cancellation and distinguishes an unavailable network', async () => {
    const transport = vi.fn<typeof fetch>().mockRejectedValueOnce(new DOMException('Aborted', 'AbortError')).mockRejectedValueOnce(new TypeError('unreachable'));
    const client = new ApiClient(transport);
    await expect(client.request('/me', schema)).rejects.toMatchObject({ name: 'AbortError' });
    await expect(client.request('/me', schema)).rejects.toMatchObject({ code: 'CLIENT_NETWORK_UNAVAILABLE' });
    expect(transport).toHaveBeenCalledTimes(2);
  });

  it('refuses an arbitrary external request path', async () => {
    const transport = vi.fn<typeof fetch>();
    await expect(new ApiClient(transport).request('https://attacker.example', schema)).rejects.toMatchObject({ code: 'CLIENT_INVALID_PATH' });
    expect(transport).not.toHaveBeenCalled();
  });
});

describe('RP logout protocol boundaries', () => {
  it('uses the fixed protocol path, invalidates CSRF on403 and does not replay automatically', async () => {
    let csrfCalls = 0; let confirms = 0;
    const transport = vi.fn<typeof fetch>().mockImplementation((input) => {
      if (inputPath(input).endsWith('/csrf')) { csrfCalls += 1; return Promise.resolve(json({ csrf_token: token })); }
      confirms += 1;
      return Promise.resolve(confirms === 1 ? json({ error: 'invalid_request' }, 403, { 'X-Request-ID': 'c09d1bb5-0018-4f9b-8d39-3f48e22f3007', 'Retry-After': '60' }) : json({ status: 'completed', redirect_to: null }));
    });
    const client = new ApiClient(transport);
    const result = z.object({ status: z.literal('completed'), redirect_to: z.string().nullable() });
    const body = { confirmation_id: 'c09d1bb5-0018-4f9b-8d39-3f48e22f3007', decision: 'logout' as const };
    await expect(client.requestProtocol('/oauth/logout/confirm', result, body)).rejects.toMatchObject({ code: 'invalid_request', status: 403, retryAfter: 60, requestId: body.confirmation_id });
    expect(confirms).toBe(1);
    await client.requestProtocol('/oauth/logout/confirm', result, body);
    expect(csrfCalls).toBe(2);
    expect(inputPath(transport.mock.calls[1]?.[0] ?? '')).toBe('/oauth/logout/confirm');
  });
});
