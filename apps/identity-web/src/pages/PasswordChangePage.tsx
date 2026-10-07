import { useState } from 'react';
import { useForm } from 'react-hook-form';
import { useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate } from 'react-router';
import { z } from 'zod';
import { api, ApiError, noContentSchema, passwordReauthSchema } from '../lib/api';
import { Password } from '../components/Password';
import { Button } from '../components/Button';
import { Status } from '../components/Status';
import { PageTitle } from './PageTitle';

export default function PasswordChangePage() {
  const navigate = useNavigate();
  const client = useQueryClient();
  const [confirmed, setConfirmed] = useState(false);
  const [requiresMfa, setRequiresMfa] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<ApiError | null>(null);
  const { register, getValues, resetField, setError, handleSubmit, formState: { errors } } = useForm<{ current: string; next: string }>();
  async function reauthenticate() {
    const password = getValues('current');
    if (!password) { setError('current', { message: '请输入当前密码' }); return; }
    setBusy(true); setFailure(null); setConfirmed(false);
    try { const result = await api.request('/me/reauth/password', passwordReauthSchema, { method: 'POST', body: { password } }); if (result.status === 'mfa_required') { setRequiresMfa(true); } else { setRequiresMfa(false); setConfirmed(true); } } catch (error) { failed(error); if (error instanceof ApiError && error.next?.required_strength === 'strong') setRequiresMfa(true); } finally { resetField('current'); setBusy(false); }
  }
  async function change(values: { current: string; next: string }) {
    const result = z.string().min(15, '密码至少需要15个字符').max(128, '密码最多128个字符').refine((value) => new TextEncoder().encode(value).length <= 512, '密码UTF-8长度最多512字节').safeParse(values.next);
    if (!result.success) { setError('next', { message: result.error.issues[0]?.message ?? '请输入有效新密码' }); resetField('next', { keepError: true }); return; }
    setBusy(true); setFailure(null);
    try { await api.request('/me/password/change', noContentSchema, { method: 'POST', body: { new_password: result.data } }); api.resetCsrf(); client.removeQueries({ queryKey: ['identity'] }); await navigate('/login', { replace: true }); } catch (error) { failed(error); if (error instanceof ApiError && error.code === 'AUTH_REAUTH_REQUIRED') { setConfirmed(false); setRequiresMfa(error.next?.required_strength === 'strong'); } } finally { resetField('next'); setBusy(false); }
  }
  function failed(error: unknown) { setFailure(error instanceof ApiError ? error : new ApiError(0, 'CLIENT_NETWORK_UNAVAILABLE', '操作暂未完成，请重试')); }
  return <><PageTitle title="修改密码" /><p className="eyebrow">ACCOUNT / PASSWORD</p><h1>修改密码</h1><p className="muted">修改后，所有设备会话与派生授权都会失效。</p><div className="component-grid"><section className="panel"><h2>确认当前密码</h2><Password label="当前密码" autoComplete="current-password" {...register('current')} error={errors.current?.message} /><Button onClick={() => { void reauthenticate(); }} loading={busy} loadingLabel="正在确认…">确认当前密码</Button>{confirmed && <Status kind="success" title="当前密码已确认" description="请在五分钟内提交新密码，服务器将再次检查认证窗口。" />}{requiresMfa && <Status kind="limited" title="还需要近期强认证" description="已有 MFA 的账号不能仅凭密码修改密码。第二因素流程将在对应任务提供。" />}</section><section className="panel"><h2>设置新密码</h2><form noValidate onSubmit={(event) => { void handleSubmit(change)(event); }}><Password label="新密码" autoComplete="new-password" disabled={!confirmed || requiresMfa} {...register('next')} error={errors.next?.message} /><Button type="submit" variant="primary" disabled={!confirmed || requiresMfa} loading={busy} loadingLabel="正在修改…">修改密码</Button></form></section></div>{failure && <Status kind={failure.status === 429 ? 'limited' : failure.status === 503 || failure.status === 0 ? 'unavailable' : 'error'} title="操作未完成" description={failure.message} requestId={failure.requestId} />}<p><Link to="/me">返回账号安全</Link></p></>;
}
