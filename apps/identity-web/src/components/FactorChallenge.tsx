import { useEffect, useRef, useState } from 'react';
import { useForm } from 'react-hook-form';
import { api, ApiError, mfaVerificationSchema } from '../lib/api';
import type { MfaVerification } from '../lib/api';
import { Button } from './Button';
import { Input } from './Input';
import { Status } from './Status';

type Challenge = { challenge_id: string; methods: readonly ('totp' | 'recovery_code')[]; expires_at: string };
export function FactorChallenge({ challenge, purpose, onComplete, onRestart, onBusyChange }: { challenge: Challenge; purpose: 'login' | 'reauthentication'; onComplete: (result: MfaVerification) => void | Promise<void>; onRestart: () => void; onBusyChange?: (busy: boolean) => void }) {
  const [method, setMethod] = useState<'totp' | 'recovery_code'>(() => challenge.methods.includes('totp') ? 'totp' : 'recovery_code');
  const [failure, setFailure] = useState<ApiError | null>(null);
  const [expired, setExpired] = useState(false);
  const [now, setNow] = useState(Date.now);
  const [retryUntil, setRetryUntil] = useState(0);
  const submitting = useRef(false);
  const currentRequest = useRef<AbortController | null>(null);
  const { register, handleSubmit, resetField, setError, formState: { errors, isSubmitting } } = useForm<{ code: string }>();
  const expiresAt = Date.parse(challenge.expires_at);
  const timedOut = !Number.isFinite(expiresAt) || now >= expiresAt;
  const retrySeconds = Math.max(0, Math.ceil((retryUntil - now) / 1000));
  const isExpired = expired || timedOut;
  useEffect(() => {
    const timer = window.setInterval(() => { setNow(Date.now()); }, 1000);
    return () => { window.clearInterval(timer); currentRequest.current?.abort(); };
  }, []);
  async function verify(values: { code: string }) {
    if (submitting.current || isExpired || retrySeconds > 0) return;
    if (method === 'totp' ? !/^\d{6}$/u.test(values.code) : !/^[A-Za-z0-9_-]{22,128}$/u.test(values.code)) { setError('code', { message: method === 'totp' ? '请输入6位数字验证码' : '请输入完整恢复码' }); resetField('code', { keepError: true }); return; }
    submitting.current = true;
    onBusyChange?.(true);
    const controller = new AbortController(); currentRequest.current = controller;
    setFailure(null);
    try {
      const result = await api.request(method === 'recovery_code' ? '/auth/mfa/recovery/verify' : purpose === 'reauthentication' ? '/me/reauth/totp' : '/auth/mfa/totp/verify', mfaVerificationSchema, { method: 'POST', body: { challenge_id: challenge.challenge_id, code: values.code }, signal: controller.signal });
      if (controller.signal.aborted) return;
      if (purpose === 'login' ? result.status !== 'authenticated' : result.status !== 'reauthenticated' || !result.strong) throw new ApiError(0, 'CLIENT_INVALID_RESPONSE', '认证结果未完成当前操作，请重新开始');
      await onComplete(result);
    } catch (error) {
      if (controller.signal.aborted) return;
      const failure = error instanceof ApiError ? error : new ApiError(0, 'CLIENT_NETWORK_UNAVAILABLE', '请求暂未完成，请重试'); setFailure(failure);
      if (failure.retryAfter !== undefined) setRetryUntil(Date.now() + failure.retryAfter * 1000);
      if (['AUTH_CHALLENGE_EXPIRED', 'AUTH_CHALLENGE_CONSUMED', 'AUTH_CHALLENGE_INVALID', 'AUTH_CHALLENGE_EXHAUSTED'].includes(failure.code)) setExpired(true);
    } finally { resetField('code'); submitting.current = false; currentRequest.current = null; onBusyChange?.(false); }
  }
  function restart() { currentRequest.current?.abort(); resetField('code'); onRestart(); }
  return <section aria-label={purpose === 'login' ? '第二因素验证' : '近期强认证'}>
    <p className="muted factor-expiry">请在{new Date(challenge.expires_at).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })}前完成验证。验证码或恢复码仅用于本次操作。</p>
    {!isExpired && <><div className="actions factor-methods">{challenge.methods.includes('totp') && <Button disabled={method === 'totp' || isSubmitting} onClick={() => { setMethod('totp'); resetField('code'); setFailure(null); }}>使用验证码</Button>}{challenge.methods.includes('recovery_code') && <Button disabled={method === 'recovery_code' || isSubmitting} onClick={() => { setMethod('recovery_code'); resetField('code'); setFailure(null); }}>使用恢复码</Button>}</div>
      <form noValidate onSubmit={(event) => { void handleSubmit(verify)(event); }}><Input label={method === 'totp' ? '验证码' : '恢复码'} inputMode={method === 'totp' ? 'numeric' : 'text'} autoComplete="one-time-code" spellCheck={false} maxLength={method === 'totp' ? 6 : 128} disabled={isSubmitting} {...register('code')} error={errors.code?.message} /><p className="muted public-auth-note">{method === 'totp' ? '打开身份验证器，输入或粘贴完整的6位验证码。' : '输入或粘贴一个已保存的恢复码。成功使用后，此码立即失效。'}</p><Button variant="primary" type="submit" loading={isSubmitting} disabled={retrySeconds > 0} loadingLabel="正在验证…">{retrySeconds > 0 ? `${String(retrySeconds)}秒后可重试` : purpose === 'login' ? '验证并登录' : '完成强认证'}</Button></form></>}
    {isExpired && <Status kind="error" title="验证已过期，请重新开始" description={failure === null ? '为保护账号，本次验证已关闭。重新登录后可获得新的验证机会。' : `${failure.message}。请重新开始验证。`} />}
    {failure && !isExpired && <Status kind={failure.status === 429 ? 'limited' : failure.status === 503 || failure.status === 0 ? 'unavailable' : 'error'} title="验证未完成" description={retrySeconds > 0 ? `${failure.message}，请等待${String(retrySeconds)}秒后重试。` : failure.message} requestId={failure.requestId} />}
    <Button onClick={restart}>重新开始{purpose === 'login' ? '登录' : '认证'}</Button>
  </section>;
}
