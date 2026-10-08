import { api, ApiError, passwordLoginSchema } from './api';
import { assertionCredential, loginOptionsSchema, requestOptions, reauthOptionsSchema, webauthnAvailable } from './webauthn';
import { mfaVerificationSchema } from './api';

export function passkeyFailure(error: unknown): ApiError {
  if (error instanceof ApiError) return error;
  if (error instanceof DOMException && ['AbortError', 'NotAllowedError'].includes(error.name)) return new ApiError(0, 'CLIENT_CANCELLED', '通行密钥验证已取消，你可以重新尝试或改用密码登录。');
  return new ApiError(0, 'CLIENT_NETWORK_UNAVAILABLE', '通行密钥暂未完成验证，请重新尝试。');
}

export async function authenticatePasskey(purpose: 'login' | 'reauthentication', signal: AbortSignal) {
  if (!webauthnAvailable()) throw new ApiError(0, 'CLIENT_PASSKEY_UNAVAILABLE', '此浏览器暂不支持通行密钥，请使用密码与已配置的验证方式。');
  const path = purpose === 'login' ? '/auth/passkeys' : '/me/reauth/passkeys';
  const options = purpose === 'login'
    ? await api.request(`${path}/options`, loginOptionsSchema, { method: 'POST', signal })
    : await api.request(`${path}/options`, reauthOptionsSchema, { method: 'POST', signal });
  signal.throwIfAborted();
  const expiresAt = Date.parse(options.expires_at);
  if (!Number.isFinite(expiresAt) || expiresAt <= Date.now()) throw new ApiError(409, 'AUTH_CHALLENGE_EXPIRED', '验证已过期，请重新开始通行密钥验证。');
  const credential = assertionCredential(await navigator.credentials.get({ publicKey: requestOptions(options.publicKey), signal }));
  signal.throwIfAborted();
  if (Date.now() >= expiresAt) throw new ApiError(409, 'AUTH_CHALLENGE_EXPIRED', '验证已过期，请重新开始通行密钥验证。');
  if (purpose === 'login') {
    const result = await api.request(`${path}/verify`, passwordLoginSchema, { method: 'POST', body: { challenge_id: options.challenge_id, credential }, signal });
    if (result.status !== 'authenticated') throw new ApiError(0, 'CLIENT_INVALID_RESPONSE', '通行密钥尚未完成登录，请重新开始。');
    return result;
  }
  const result = await api.request(`${path}/verify`, mfaVerificationSchema, { method: 'POST', body: { challenge_id: options.challenge_id, credential }, signal });
  if (result.status !== 'reauthenticated' || !result.strong) throw new ApiError(0, 'CLIENT_INVALID_RESPONSE', '通行密钥尚未完成强认证，请重新开始。');
  return result;
}
