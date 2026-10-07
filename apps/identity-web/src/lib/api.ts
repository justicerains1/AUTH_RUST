import { z } from 'zod';

const uuid = z.uuid();
const utc = z.iso.datetime({ offset: true });
const nextSchema = z.object({ status: z.literal('reauth_required'), required_strength: z.enum(['password', 'strong']), methods: z.array(z.enum(['password', 'totp', 'passkey', 'recovery_code'])) });
const errorSchema = z.object({ error: z.object({ code: z.string().min(1), message: z.string(), request_id: uuid }), next: nextSchema.optional() });
const csrfSchema = z.object({ csrf_token: z.string().min(43).max(512) });
export const userSchema = z.object({ id: uuid, sub: uuid, email: z.email(), email_verified: z.boolean(), display_name: z.string().optional(), status: z.enum(['active', 'disabled']), created_at: utc });
export const sessionSchema = z.object({ id: uuid, amr: z.array(z.enum(['pwd', 'otp', 'rcv', 'user', 'hwk'])).min(1), auth_time: utc, strong_at: utc.nullable(), expires_at: utc, created_at: utc, user_agent: z.string().max(256).optional(), current: z.boolean() });
export const meSchema = z.object({
  user: userSchema,
  session: sessionSchema,
  security: z.object({ totp_enabled: z.boolean(), passkey_count: z.number().int().min(0).max(10), recovery_codes_remaining: z.number().int().min(0).max(10), is_admin: z.boolean(), admin_binding_only: z.boolean() }),
});
export const passwordLoginSchema = z.discriminatedUnion('status', [
  z.object({ status: z.literal('authenticated'), user: userSchema, session: sessionSchema, csrf_token: z.string().min(43).max(512) }),
  z.object({ status: z.literal('mfa_required'), challenge_id: uuid, purpose: z.literal('login'), methods: z.array(z.enum(['totp', 'recovery_code'])).min(1).max(2), expires_at: utc }),
]);
export type PasswordLogin = z.infer<typeof passwordLoginSchema>;
export const sessionPageSchema = z.object({ items: z.array(sessionSchema).max(100), next_cursor: z.string().max(2048).nullable() });
export const noContentSchema = z.undefined();
export const passwordResetSchema = z.object({ status: z.literal('password_reset'), next: z.literal('login') });
export const passwordReauthSchema = z.discriminatedUnion('status', [
  z.object({ status: z.literal('reauthenticated'), reauthenticated_at: utc, valid_until: utc, strong: z.boolean(), amr: z.array(z.enum(['pwd', 'otp', 'rcv', 'user', 'hwk'])).min(1) }),
  z.object({ status: z.literal('mfa_required'), challenge_id: uuid, purpose: z.literal('reauthentication'), methods: z.array(z.enum(['totp', 'recovery_code'])).min(1).max(2), expires_at: utc }),
]);
export const mfaVerificationSchema = z.discriminatedUnion('status', [
  z.object({ status: z.literal('authenticated'), user: userSchema, session: sessionSchema, csrf_token: z.string().min(43).max(512) }),
  z.object({ status: z.literal('reauthenticated'), reauthenticated_at: utc, valid_until: utc, strong: z.boolean(), amr: z.array(z.enum(['pwd', 'otp', 'rcv', 'user', 'hwk'])).min(1) }),
]);
export const totpEnrollmentSchema = z.object({ challenge_id: uuid, purpose: z.literal('totp_enrollment'), secret: z.string().min(32).max(128), otpauth_uri: z.string().startsWith('otpauth://totp/'), expires_at: utc });
export const recoveryCodesSchema = z.object({ codes: z.array(z.string().min(22).max(128)).length(10), shown_once: z.literal(true) });
export const totpCompletedSchema = z.object({ status: z.literal('totp_enabled'), recovery_codes: recoveryCodesSchema });
export type MfaVerification = z.infer<typeof mfaVerificationSchema>;
export type TotpEnrollment = z.infer<typeof totpEnrollmentSchema>;
export const authorizationTransactionSchema = z.object({ id: z.uuid(), client: z.object({ client_id: z.string().min(1).max(128), name: z.string().min(1).max(100) }), requested_scopes: z.array(z.enum(['openid', 'profile', 'email'])).min(1), previously_approved_scopes: z.array(z.enum(['openid', 'profile', 'email'])), expires_at: z.iso.datetime({ offset: true }), status: z.enum(['login_required', 'consent_required']) });
export const authorizationDecisionSchema = z.object({ status: z.literal('completed'), redirect_to: z.url() });
export type Me = z.infer<typeof meSchema>;
export const acceptedSchema = z.object({ status: z.literal('accepted'), message: z.string() });
export const emailVerifiedSchema = z.object({ status: z.literal('verified') });

export class ApiError extends Error {
  readonly status: number;
  readonly code: string;
  readonly requestId: string | undefined;
  readonly retryAfter: number | undefined;
  readonly next: z.infer<typeof nextSchema> | undefined;

  constructor(status: number, code: string, message: string, options: { requestId?: string; retryAfter?: number; next?: z.infer<typeof nextSchema> } = {}) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.code = code;
    this.requestId = options.requestId;
    this.retryAfter = options.retryAfter;
    this.next = options.next;
  }
}

type RequestOptions = { method?: 'GET' | 'POST' | 'PATCH' | 'DELETE'; body?: unknown; signal?: AbortSignal };

/** Credentials stay in memory. State-changing requests never replay automatically. */
export class ApiClient {
  private csrf: string | undefined;
  private csrfRequest: Promise<string> | undefined;
  private generation = 0;
  private readonly transport: typeof fetch;

  constructor(transport: typeof fetch = fetch) { this.transport = transport.bind(globalThis); }

  resetCsrf(): void { this.generation += 1; this.csrf = undefined; this.csrfRequest = undefined; }

  acceptRotatedCsrf(token: string): void {
    this.resetCsrf();
    this.csrf = csrfSchema.parse({ csrf_token: token }).csrf_token;
  }

  private async getCsrf(): Promise<string> {
    if (this.csrf !== undefined) return this.csrf;
    if (this.csrfRequest !== undefined) return this.csrfRequest;
    const generation = this.generation;
    const pending = this.request('/auth/csrf', csrfSchema).then(({ csrf_token }) => {
      if (generation === this.generation) this.csrf = csrf_token;
      return csrf_token;
    });
    this.csrfRequest = pending;
    try { return await pending; } finally { if (generation === this.generation) this.csrfRequest = undefined; }
  }

  async request<T>(path: string, schema: z.ZodType<T>, options: RequestOptions = {}): Promise<T> {
    if (!/^\/(?:auth|me|admin|oauth)(?:\/|$)/u.test(path) || path.includes('#') || path.startsWith('//')) {
      throw new ApiError(0, 'CLIENT_INVALID_PATH', '请求地址无效');
    }
    const resolved = new URL(`/api/v1${path}`, 'https://identity.invalid');
    if (!resolved.pathname.startsWith('/api/v1/') || resolved.origin !== 'https://identity.invalid') {
      throw new ApiError(0, 'CLIENT_INVALID_PATH', '请求地址无效');
    }
    const method = options.method ?? 'GET';
    const headers = new Headers({ Accept: 'application/json' });
    if (method !== 'GET') {
      options.signal?.throwIfAborted();
      headers.set('X-CSRF-Token', await this.getCsrf());
      options.signal?.throwIfAborted();
    }
    if (options.body !== undefined) headers.set('Content-Type', 'application/json');
    let response: Response;
    try {
      response = await this.transport(`/api/v1${path}`, {
        method, headers, credentials: 'same-origin', cache: 'no-store',
        ...(options.body === undefined ? {} : { body: JSON.stringify(options.body) }),
        ...(options.signal === undefined ? {} : { signal: options.signal }),
      });
    } catch (error) {
      if (options.signal?.aborted === true || (error instanceof DOMException && error.name === 'AbortError')) throw error;
      throw new ApiError(0, 'CLIENT_NETWORK_UNAVAILABLE', '网络暂不可用，请重试');
    }
    if (response.status === 401) this.resetCsrf();
    if (!response.ok) {
      let value: unknown;
      try { value = await response.json(); } catch { value = undefined; }
      const parsed = errorSchema.safeParse(value);
      const retry = response.headers.get('Retry-After');
      const retryAfter = retry !== null && /^\d+$/u.test(retry) ? Number(retry) : undefined;
      if (parsed.success) {
        if (parsed.data.error.code === 'AUTH_CSRF_INVALID') this.resetCsrf();
        throw new ApiError(response.status, parsed.data.error.code, parsed.data.error.message, {
          requestId: parsed.data.error.request_id,
          ...(retryAfter === undefined ? {} : { retryAfter }),
          ...(parsed.data.next === undefined ? {} : { next: parsed.data.next }),
        });
      }
      throw new ApiError(response.status, 'CLIENT_INVALID_RESPONSE', '服务暂不可用，请稍后重试');
    }
    let value: unknown;
    try { value = response.status === 204 ? undefined : await response.json(); } catch {
      throw new ApiError(response.status, 'CLIENT_INVALID_RESPONSE', '服务返回了无法处理的结果');
    }
    const parsed = schema.safeParse(value);
    if (!parsed.success) throw new ApiError(response.status, 'CLIENT_INVALID_RESPONSE', '服务返回了无法处理的结果');
    return parsed.data;
  }
}

export const api = new ApiClient();
