import { lazy, Suspense, useState } from 'react';
import { useForm } from 'react-hook-form';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router';
import { api, ApiError, meSchema, noContentSchema, passwordReauthSchema, recoveryCodesSchema, totpCompletedSchema, totpEnrollmentSchema } from '../lib/api';
import type { TotpEnrollment } from '../lib/api';
import { meQueryKey } from '../lib/query';
import { Button } from '../components/Button';
import { Password } from '../components/Password';
import { Input } from '../components/Input';
import { Status } from '../components/Status';
import { ConfirmDialog } from '../components/ConfirmDialog';
import { FactorChallenge } from '../components/FactorChallenge';
import { RecoveryCodes } from '../components/RecoveryCodes';
import { PageTitle } from './PageTitle';
import { PasskeyReauth } from '../components/PasskeyReauth';

const TotpQrCode = lazy(() => import('../components/TotpQrCode'));
type ReauthChallenge = { challenge_id: string; methods: ('totp' | 'recovery_code')[]; expires_at: string };

export default function MfaPage() {
  const client = useQueryClient();
  const query = useQuery({ queryKey: meQueryKey, queryFn: ({ signal }) => api.request('/me', meSchema, { signal }) });
  const [failure, setFailure] = useState<ApiError | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirmed, setConfirmed] = useState(false);
  const [challenge, setChallenge] = useState<ReauthChallenge | null>(null);
  const [enrollment, setEnrollment] = useState<TotpEnrollment | null>(null);
  const [codes, setCodes] = useState<string[]>([]);
  const { register, getValues, resetField, setError, formState: { errors } } = useForm<{ password: string; code: string }>();
  async function reauthenticate() {
    const password = getValues('password'); if (!password) { setError('password', { message: '请输入当前密码' }); return; }
    setBusy(true); setFailure(null); setConfirmed(false);
    try { const result = await api.request('/me/reauth/password', passwordReauthSchema, { method: 'POST', body: { password } }); if (result.status === 'mfa_required') setChallenge(result); else setConfirmed(true); } catch (error) { failed(error); } finally { resetField('password'); setBusy(false); }
  }
  async function enroll() { setBusy(true); setFailure(null); try { setEnrollment(await api.request('/me/mfa/totp/enrollment', totpEnrollmentSchema, { method: 'POST' })); } catch (error) { failed(error); } finally { setBusy(false); } }
  async function confirmEnrollment() {
    if (!enrollment) return; const code = getValues('code'); if (!/^\d{6}$/u.test(code)) { setError('code', { message: '请输入6位数字验证码' }); return; }
    setBusy(true); setFailure(null);
    try { const result = await api.request('/me/mfa/totp/enrollment/confirm', totpCompletedSchema, { method: 'POST', body: { challenge_id: enrollment.challenge_id, code } }); setCodes(result.recovery_codes.codes); setEnrollment(null); setConfirmed(false); await client.invalidateQueries({ queryKey: ['identity'] }); } catch (error) { failed(error); if (error instanceof ApiError && ['AUTH_CHALLENGE_EXPIRED', 'AUTH_CHALLENGE_CONSUMED'].includes(error.code)) setEnrollment(null); } finally { resetField('code'); setBusy(false); }
  }
  async function disable() { try { await api.request('/me/mfa/totp', noContentSchema, { method: 'DELETE' }); setCodes([]); setConfirmed(false); await client.invalidateQueries({ queryKey: ['identity'] }); } catch (error) { failed(error); throw error; } }
  async function regenerate() { try { const result = await api.request('/me/mfa/recovery-codes/regenerate', recoveryCodesSchema, { method: 'POST' }); setCodes(result.codes); await client.invalidateQueries({ queryKey: ['identity'] }); } catch (error) { failed(error); throw error; } }
  function failed(error: unknown) { setFailure(error instanceof ApiError ? error : new ApiError(0, 'CLIENT_NETWORK_UNAVAILABLE', '操作暂未完成，请重试')); }
  return <><PageTitle title="双因素验证" /><p className="eyebrow">ACCOUNT / MFA</p><h1>双因素验证</h1><p><Link to="/me">返回账号安全</Link></p>{failure && <Status kind={failure.status === 429 ? 'limited' : failure.status === 503 || failure.status === 0 ? 'unavailable' : 'error'} title="操作未完成" description={failure.message} requestId={failure.requestId} />}<div className="component-grid"><section className="panel"><h2>近期认证</h2><p>首次绑定需确认密码，已有因素的安全变更还需要第二因素。</p><Password label="当前密码" {...register('password')} error={errors.password?.message} /><Button loading={busy} loadingLabel="正在确认…" onClick={() => { void reauthenticate(); }}>确认当前密码</Button>{(query.data?.security.passkey_count ?? 0)>0 && <PasskeyReauth onConfirmed={async()=>{setConfirmed(true);await client.invalidateQueries({queryKey:['identity']});}}/>}{confirmed && <Status kind="success" title="近期认证已完成" />}{challenge && <FactorChallenge challenge={challenge} purpose="reauthentication" onComplete={async (result) => { if (result.status !== 'reauthenticated' || !result.strong) throw new ApiError(0, 'CLIENT_INVALID_RESPONSE', '此结果不能完成强认证'); setChallenge(null); setConfirmed(true); await client.invalidateQueries({ queryKey: ['identity'] }); }} onRestart={() => { setChallenge(null); setConfirmed(false); }} />}</section><section className="panel"><h2>TOTP设置</h2><p>{query.data?.security.totp_enabled ? 'TOTP已启用' : 'TOTP未启用'}</p>{!query.data?.security.totp_enabled && !enrollment && <Button disabled={!confirmed} loading={busy} onClick={() => { void enroll(); }}>开始绑定TOTP</Button>}{enrollment && <><p>使用认证器扫描二维码，或手动输入密钥。此密钥仅显示在当前页面。</p><Suspense fallback={<p>正在生成二维码…</p>}><TotpQrCode uri={enrollment.otpauth_uri} /></Suspense><p>设置密钥：<code className="setup-secret">{enrollment.secret}</code></p><Input label="验证码" inputMode="numeric" autoComplete="one-time-code" {...register('code')} error={errors.code?.message} /><Button loading={busy} loadingLabel="正在启用…" onClick={() => { void confirmEnrollment(); }}>确认启用TOTP</Button><Button onClick={() => { setEnrollment(null); resetField('code'); }}>取消绑定</Button></>}{query.data?.security.totp_enabled && <ConfirmDialog trigger={<Button variant="danger">关闭TOTP</Button>} title="关闭TOTP？" description="关闭后不再要求TOTP登录；操作需要最近五分钟内的强认证。" confirmLabel="确认关闭TOTP" onConfirm={disable} />}</section><section className="panel"><h2>恢复码</h2><p>每个恢复码只能使用一次，重新生成会立即作废旧码。</p><ConfirmDialog trigger={<Button>重新生成恢复码</Button>} title="重新生成恢复码？" description="旧恢复码将全部失效，新码只显示一次；需要近期强认证。" confirmLabel="确认重新生成" onConfirm={regenerate} /></section></div>{codes.length > 0 && <RecoveryCodes codes={codes} onClose={() => { setCodes([]); }} />}</>;
}
