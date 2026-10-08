import { z } from 'zod';
import { api, noContentSchema, userSchema } from './api';

const scopes = z.array(z.enum(['openid', 'profile', 'email'])).min(1).max(3);
const timestamp = z.iso.datetime({ offset: true });
export const adminClientSchema = z.strictObject({ id: z.uuid(), client_id: z.string().min(1).max(128), name: z.string().min(1).max(100), enabled: z.boolean(), allowed_scopes: scopes, redirect_uris: z.array(z.url()).min(1).max(20), post_logout_redirect_uris: z.array(z.url()).max(20), created_at: timestamp });
export const adminMemberSchema = z.strictObject({ user_id: z.uuid(), email: z.email(), enabled: z.boolean(), created_at: timestamp });
const auditEvents = ['auth.session_created', 'auth.password_failed', 'email.verified', 'password.reset_completed', 'password.changed', 'auth.reauthenticated', 'mfa.totp_enrolled', 'mfa.totp_removed', 'mfa.recovery_code_used', 'mfa.recovery_codes_regenerated', 'passkey.registered', 'passkey.renamed', 'passkey.removed', 'auth.session_revoked', 'auth.sessions_revoked_all', 'oauth.consent_granted', 'oauth.consent_denied', 'oauth.refresh_rotated', 'oauth.refresh_replay_detected', 'oauth.grant_revoked', 'admin.user_disabled', 'admin.user_sessions_revoked', 'oauth.client_created', 'oauth.client_updated', 'oauth.client_secret_rotated', 'admin.member_granted', 'admin.member_removed', 'identity.registration_requested', 'email.verification_requested', 'auth.challenge_failed', 'auth.challenge_exhausted', 'password.reset_requested', 'oauth.code_consumed', 'oauth.client_disabled', 'admin.initialized', 'admin.last_member_removal_denied', 'admin.user_enabled', 'system.rate_limited', 'system.dependency_unavailable', 'email.delivery_failed', 'email.delivery_succeeded', 'keys.signing_rotated', 'keys.encryption_rotated'] as const;
export const auditEventSchema = z.strictObject({ id: z.uuid(), event: z.enum(auditEvents), actor_user_id: z.uuid().nullable(), target_type: z.enum(['user', 'session', 'grant', 'client', 'admin_member', 'credential', 'challenge', 'outbox']), target_id: z.string().max(128).nullable(), result: z.enum(['success', 'failure', 'denied']), request_id: z.uuid(), source: z.string().max(128), occurred_at: timestamp });
const page = <T extends z.ZodType>(schema: T) => z.strictObject({ items: z.array(schema).max(100), next_cursor: z.string().max(2048).nullable() });
export const adminUserPageSchema = page(userSchema);
export const adminClientPageSchema = page(adminClientSchema);
export const adminMemberPageSchema = page(adminMemberSchema);
export const adminAuditPageSchema = page(auditEventSchema);
export const clientSecretOnceSchema = z.strictObject({ client: adminClientSchema, client_secret: z.string().regex(/^[A-Za-z0-9_-]{43}$/u), shown_once: z.literal(true) });
export const clientSecretRotationSchema = z.strictObject({ client_id: z.string().min(1).max(128), client_secret: z.string().regex(/^[A-Za-z0-9_-]{43}$/u), shown_once: z.literal(true) });
export const revokedSessionsSchema = z.strictObject({ status: z.literal('revoked'), revoked_session_count: z.number().int().nonnegative() });
export type AdminClient = z.infer<typeof adminClientSchema>;
export type AdminMember = z.infer<typeof adminMemberSchema>;
export type ClientInput = { name: string; allowed_scopes: ('openid' | 'profile' | 'email')[]; redirect_uris: string[]; post_logout_redirect_uris: string[] };

export function adminQuery(parameters: { cursor?: string | undefined; email?: string | undefined; status?: 'active' | 'disabled' | undefined; from?: string | undefined; to?: string | undefined }) {
  const query = new URLSearchParams({ limit: '20' });
  for (const [key, value] of Object.entries(parameters)) if (value !== undefined && value !== '') query.set(key, value);
  return query.toString();
}
export const adminApi = {
  users: (parameters: Parameters<typeof adminQuery>[0], signal?: AbortSignal) => api.request(`/admin/users?${adminQuery(parameters)}`, adminUserPageSchema, signal === undefined ? {} : { signal }),
  user: (id: string, signal?: AbortSignal) => api.request(`/admin/users/${z.uuid().parse(id)}`, userSchema, signal === undefined ? {} : { signal }),
  setStatus: (id: string, status: 'active' | 'disabled') => api.request(`/admin/users/${z.uuid().parse(id)}/status`, userSchema, { method: 'PATCH', body: { status } }),
  revokeSessions: (id: string) => api.request(`/admin/users/${z.uuid().parse(id)}/revoke-sessions`, revokedSessionsSchema, { method: 'POST' }),
  clients: (cursor?: string, signal?: AbortSignal) => api.request(`/admin/clients?${adminQuery({ cursor })}`, adminClientPageSchema, signal === undefined ? {} : { signal }),
  createClient: (body: ClientInput) => api.request('/admin/clients', clientSecretOnceSchema, { method: 'POST', body }),
  updateClient: (id: string, body: Partial<ClientInput> & { enabled?: boolean }) => api.request(`/admin/clients/${z.uuid().parse(id)}`, adminClientSchema, { method: 'PATCH', body }),
  rotateSecret: (id: string) => api.request(`/admin/clients/${z.uuid().parse(id)}/rotate-secret`, clientSecretRotationSchema, { method: 'POST' }),
  members: (cursor?: string, signal?: AbortSignal) => api.request(`/admin/members?${adminQuery({ cursor })}`, adminMemberPageSchema, signal === undefined ? {} : { signal }),
  grantMember: (user_id: string) => api.request('/admin/members', adminMemberSchema, { method: 'POST', body: { user_id: z.uuid().parse(user_id) } }),
  removeMember: (id: string) => api.request(`/admin/members/${z.uuid().parse(id)}`, noContentSchema, { method: 'DELETE' }),
  audit: (parameters: Parameters<typeof adminQuery>[0], signal?: AbortSignal) => api.request(`/admin/audit-events?${adminQuery(parameters)}`, adminAuditPageSchema, signal === undefined ? {} : { signal }),
};
/** One-time client secrets use direct requests and local result state, never Query/mutation cache. */
