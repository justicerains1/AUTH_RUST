import { useState } from 'react';
import { useForm } from 'react-hook-form';
import { api, ApiError, mfaVerificationSchema } from '../lib/api';
import type { MfaVerification } from '../lib/api';
import { Button } from './Button';
import { Input } from './Input';
import { Status } from './Status';

type Challenge = { challenge_id: string; methods: readonly ('totp' | 'recovery_code')[]; expires_at: string };
export function FactorChallenge({ challenge, purpose, onComplete, onRestart }: { challenge: Challenge; purpose: 'login' | 'reauthentication'; onComplete: (result: MfaVerification) => void | Promise<void>; onRestart: () => void }) {
  const [method, setMethod] = useState<'totp' | 'recovery_code'>(() => challenge.methods.includes('totp') ? 'totp' : 'recovery_code');
  const [failure, setFailure] = useState<ApiError | null>(null);
  const [expired, setExpired] = useState(false);
  const { register, handleSubmit, resetField, setError, formState: { errors, isSubmitting } } = useForm<{ code: string }>();
  async function verify(values: { code: string }) {
    if (method === 'totp' ? !/^\d{6}$/u.test(values.code) : !/^[A-Za-z0-9_-]{22,128}$/u.test(values.code)) { setError('code', { message: method === 'totp' ? '请输入6位数字验证码' : '请输入完整恢复码' }); resetField('code', { keepError: true }); return; }
    setFailure(null);
    try { const result = await api.request(method === 'recovery_code' ? '/auth/mfa/recovery/verify' : purpose === 'reauthentication' ? '/me/reauth/totp' : '/auth/mfa/totp/verify', mfaVerificationSchema, { method: 'POST', body: { challenge_id: challenge.challenge_id, code: values.code } }); await onComplete(result); } catch (error) { const failure = error instanceof ApiError ? error : new ApiError(0, 'CLIENT_NETWORK_UNAVAILABLE', '请求暂未完成，请重试'); setFailure(failure); if (['AUTH_CHALLENGE_EXPIRED', 'AUTH_CHALLENGE_CONSUMED', 'AUTH_CHALLENGE_INVALID'].includes(failure.code)) setExpired(true); } finally { resetField('code'); }
  }
  return <section aria-label={purpose === 'login' ? '第二因素验证' : '近期强认证'}><p className="muted">挑战到期：{new Date(challenge.expires_at).toLocaleString('zh-CN')}。验证码或恢复码只用于本次验证。</p>{!expired && <><div className="actions">{challenge.methods.includes('totp') && <Button disabled={method === 'totp'} onClick={() => { setMethod('totp'); resetField('code'); }}>使用验证码</Button>}{challenge.methods.includes('recovery_code') && <Button disabled={method === 'recovery_code'} onClick={() => { setMethod('recovery_code'); resetField('code'); }}>使用恢复码</Button>}</div><form noValidate onSubmit={(event) => { void handleSubmit(verify)(event); }}><Input label={method === 'totp' ? '验证码' : '恢复码'} inputMode={method === 'totp' ? 'numeric' : 'text'} autoComplete="one-time-code" {...register('code')} error={errors.code?.message} /><Button variant="primary" type="submit" loading={isSubmitting} loadingLabel="正在验证…">{purpose === 'login' ? '验证并登录' : '完成强认证'}</Button></form></>}{failure && <Status kind={failure.status === 429 ? 'limited' : failure.status === 503 || failure.status === 0 ? 'unavailable' : 'error'} title={expired ? '挑战已失效，请重新开始' : '验证未完成'} description={failure.message} requestId={failure.requestId} />}<Button onClick={onRestart}>重新开始{purpose === 'login' ? '登录' : '认证'}</Button></section>;
}
