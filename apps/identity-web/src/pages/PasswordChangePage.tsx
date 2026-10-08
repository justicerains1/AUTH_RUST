import { useState } from 'react';
import { useForm } from 'react-hook-form';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from 'react-router';
import { z } from 'zod';
import { api, meSchema, noContentSchema } from '../lib/api';
import type { ApiError } from '../lib/api';
import { asApiError, useRetryAfter } from '../lib/auth-feedback';
import { useReauthentication } from '../lib/reauthentication';
import { meQueryKey } from '../lib/query';
import { Password } from '../components/Password';
import { Button } from '../components/Button';
import { Status } from '../components/Status';
import { AccountLayout } from '../components/AccountLayout';
import { AccountAction } from '../components/AccountAction';
import { ReauthenticationDialog } from '../components/ReauthenticationDialog';
import { RequestFeedback } from '../components/RequestFeedback';

const passwordSchema = z.string().refine((value) => Array.from(value).length >= 15, '密码至少需要15个字符').refine((value) => Array.from(value).length <= 128 && new TextEncoder().encode(value).length <= 512, '密码长度超出限制');
export default function PasswordChangePage() {
  const navigate = useNavigate();
  const client = useQueryClient();
  const identity = useQuery({ queryKey: meQueryKey, queryFn: ({ signal }) => api.request('/me', meSchema, { signal }) });
  const reauthentication = useReauthentication(identity.data);
  const retry = useRetryAfter();
  const [failure, setFailure] = useState<ApiError | null>(null);
  const { register, getValues, resetField, setError, formState: { errors } } = useForm<{ next: string }>();
  function requestAuthentication(error?: ApiError) { resetField('next'); reauthentication.request(error); }
  async function change() {
    const parsed = passwordSchema.safeParse(getValues('next'));
    if (!parsed.success) { setError('next', { message: parsed.error.issues[0]?.message ?? '请输入有效新密码' }); resetField('next', { keepError: true }); throw new Error('Password input invalid'); }
    try { await api.request('/me/password/change', noContentSchema, { method: 'POST', body: { new_password: parsed.data } }); api.resetCsrf(); client.removeQueries({ queryKey: ['identity'] }); await navigate('/login', { replace: true }); }
    catch (error) { const detail = asApiError(error); setFailure(detail); retry.start(detail); throw detail; }
    finally { resetField('next'); }
  }
  return <AccountLayout title="修改密码" description="修改后，所有设备会话与派生授权都会失效。"><section className="panel"><h2>确认当前身份</h2><p>先完成当前账号所需的近期认证，再设置新密码。</p><Button onClick={() => { requestAuthentication(); }}>确认当前身份</Button>{reauthentication.confirmed && <Status kind="success" title="近期认证已完成" description="请设置新密码，并明确确认修改。服务器会再次检查认证窗口。" />}</section><section className="panel"><h2>设置新密码</h2><Password label="新密码" autoComplete="new-password" disabled={!reauthentication.confirmed} {...register('next')} error={errors.next?.message} aria-describedby="change-password-hint" /><p className="field-hint password-hint" id="change-password-hint">15 至 128 个字符，允许空格和粘贴。</p><AccountAction trigger={<Button variant="primary" disabled={!reauthentication.confirmed || retry.remaining > 0}>修改密码</Button>} title="确认修改密码？" description="所有设备及相关应用授权将失效。完成后请使用新密码重新登录。" confirmLabel="确认修改密码" onConfirm={change} onReauthentication={requestAuthentication} /></section><RequestFeedback error={failure} title="操作未完成" remaining={retry.remaining} /><ReauthenticationDialog open={reauthentication.open} onOpenChange={reauthentication.setOpen} requiredStrength={reauthentication.requiredStrength} methods={reauthentication.methods} onConfirmed={reauthentication.complete} /></AccountLayout>;
}
