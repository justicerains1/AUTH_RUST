import { lazy, Suspense, useEffect, useRef, useState } from 'react';
import { useForm } from 'react-hook-form';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, meSchema, noContentSchema, recoveryCodesSchema, totpCompletedSchema, totpEnrollmentSchema } from '../lib/api';
import type { ApiError, TotpEnrollment } from '../lib/api';
import { asApiError, useRetryAfter } from '../lib/auth-feedback';
import { useReauthentication } from '../lib/reauthentication';
import { meQueryKey } from '../lib/query';
import { Button } from '../components/Button';
import { Input } from '../components/Input';
import { Status } from '../components/Status';
import { AccountAction } from '../components/AccountAction';
import { AccountLayout } from '../components/AccountLayout';
import { RecoveryCodes } from '../components/RecoveryCodes';
import { ReauthenticationDialog } from '../components/ReauthenticationDialog';
import { RequestFeedback } from '../components/RequestFeedback';

const TotpQrCode = lazy(() => import('../components/TotpQrCode'));
export default function MfaPage() {
  const client = useQueryClient();
  const query = useQuery({ queryKey: meQueryKey, queryFn: ({ signal }) => api.request('/me', meSchema, { signal }) });
  const reauthentication = useReauthentication(query.data);
  const retry = useRetryAfter();
  const active = useRef(false);
  const [failure, setFailure] = useState<ApiError | null>(null);
  const [busy, setBusy] = useState(false);
  const [enrollment, setEnrollment] = useState<TotpEnrollment | null>(null);
  const [codes, setCodes] = useState<string[]>([]);
  const [enrollmentExpired, setEnrollmentExpired] = useState(false);
  const { register, getValues, resetField, setError, formState: { errors } } = useForm<{ code: string }>();
  useEffect(() => {
    if (enrollment === null) return;
    const timeout = window.setTimeout(() => { setEnrollment(null); setEnrollmentExpired(true); resetField('code'); }, Math.max(0, Date.parse(enrollment.expires_at) - Date.now()));
    return () => { window.clearTimeout(timeout); };
  }, [enrollment, resetField]);
  async function refresh() { await client.invalidateQueries({ queryKey: ['identity', 'me'] }); }
  function failed(error: unknown) { const detail = asApiError(error); setFailure(detail); retry.start(detail); if (detail.code === 'AUTH_REAUTH_REQUIRED') reauthentication.request(detail); }
  async function enroll() {
    if (active.current || retry.remaining > 0) return;
    active.current = true; setBusy(true); setFailure(null); setEnrollmentExpired(false);
    try { setEnrollment(await api.request('/me/mfa/totp/enrollment', totpEnrollmentSchema, { method: 'POST' })); }
    catch (error) { failed(error); } finally { active.current = false; setBusy(false); }
  }
  async function confirmEnrollment() {
    if (enrollment === null || active.current || enrollmentExpired || retry.remaining > 0) return;
    const code = getValues('code'); if (!/^\d{6}$/u.test(code)) { setError('code', { message: '请输入6位数字验证码' }); resetField('code', { keepError: true }); return; }
    active.current = true; setBusy(true); setFailure(null);
    try { const result = await api.request('/me/mfa/totp/enrollment/confirm', totpCompletedSchema, { method: 'POST', body: { challenge_id: enrollment.challenge_id, code } }); setCodes(result.recovery_codes.codes); setEnrollment(null); await refresh(); }
    catch (error) { failed(error); const detail = asApiError(error); if (['AUTH_CHALLENGE_EXPIRED', 'AUTH_CHALLENGE_CONSUMED', 'AUTH_CHALLENGE_INVALID'].includes(detail.code)) setEnrollment(null); }
    finally { resetField('code'); active.current = false; setBusy(false); }
  }
  async function disable() { await api.request('/me/mfa/totp', noContentSchema, { method: 'DELETE' }); setCodes([]); await refresh(); }
  async function regenerate() { const result = await api.request('/me/mfa/recovery-codes/regenerate', recoveryCodesSchema, { method: 'POST' }); setCodes(result.codes); await refresh(); }
  if (query.isPending) return <Status kind="loading" title="正在加载双因素设置…" />;
  if (query.isError) return <Status kind="unavailable" title="暂时无法加载双因素设置" onRetry={() => { void query.refetch(); }} />;
  return <AccountLayout title="双因素验证" description="为密码登录增加一道保护，并在需要时使用一次性恢复码。"><section className="panel"><h2>确认当前身份</h2><p>首次绑定需要确认密码；已有因素的安全变更还需要第二因素。</p><Button disabled={busy} onClick={() => { reauthentication.request(); }}>确认当前身份</Button>{reauthentication.confirmed && <Status kind="success" title="近期认证已完成" description="请再次选择并确认你要完成的操作。" />}</section><section className="panel"><h2>身份验证器</h2><p>{query.data.security.totp_enabled ? 'TOTP已启用' : 'TOTP未启用'}</p>{!query.data.security.totp_enabled && enrollment === null && <Button disabled={!reauthentication.confirmed || retry.remaining > 0} loading={busy} onClick={() => { void enroll(); }}>开始绑定TOTP</Button>}{enrollmentExpired && <Status kind="error" title="绑定已过期" description="本次设置密钥已清除。确认身份后请重新开始绑定。" />}{enrollment !== null && <><p>使用认证器扫描二维码，或手动输入密钥。此密钥仅显示在当前页面。</p><Suspense fallback={<p>正在生成二维码…</p>}><TotpQrCode uri={enrollment.otpauth_uri} /></Suspense><p>设置密钥：<code className="setup-secret">{enrollment.secret}</code></p><p className="muted">绑定到期：{new Date(enrollment.expires_at).toLocaleTimeString('zh-CN')}</p><Input label="验证码" inputMode="numeric" autoComplete="one-time-code" spellCheck={false} maxLength={6} {...register('code')} error={errors.code?.message} /><Button disabled={retry.remaining > 0} loading={busy} loadingLabel="正在启用…" onClick={() => { void confirmEnrollment(); }}>确认启用TOTP</Button><Button disabled={busy} onClick={() => { setEnrollment(null); resetField('code'); }}>取消绑定</Button></>}{query.data.security.totp_enabled && <AccountAction trigger={<Button variant="danger">关闭TOTP</Button>} title="关闭TOTP？" description={query.data.security.passkey_count > 0 ? "关闭后不再要求TOTP验证，已有通行密钥与恢复码继续保留；此操作需要近期强认证。" : "关闭后不再要求TOTP验证，当前恢复码集合也将失效；此操作需要近期强认证。"} confirmLabel="确认关闭TOTP" onConfirm={disable} onReauthentication={reauthentication.request} />}</section><section className="panel"><h2>恢复码</h2><p>剩余 {query.data.security.recovery_codes_remaining} 个。每个只能使用一次，重新生成会立即作废旧码。</p><AccountAction trigger={<Button disabled={!query.data.security.totp_enabled && query.data.security.passkey_count === 0}>重新生成恢复码</Button>} title="重新生成恢复码？" description="旧恢复码将全部失效，新码只显示一次。请准备在可信位置保存。" confirmLabel="确认重新生成" onConfirm={regenerate} onReauthentication={reauthentication.request} /></section><RequestFeedback error={failure} title="操作未完成" remaining={retry.remaining} />{codes.length > 0 && <RecoveryCodes codes={codes} onClose={() => { setCodes([]); }} />}<ReauthenticationDialog open={reauthentication.open} onOpenChange={reauthentication.setOpen} requiredStrength={reauthentication.requiredStrength} methods={reauthentication.methods} onConfirmed={reauthentication.complete} /></AccountLayout>;
}
