import { useRef, useState } from 'react';
import { useForm } from 'react-hook-form';
import { useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate } from 'react-router';
import { z } from 'zod';
import { api, acceptedSchema, passwordResetSchema, ApiError } from '../lib/api';
import { Button } from '../components/Button';
import { Input } from '../components/Input';
import { Password } from '../components/Password';
import { Status } from '../components/Status';
import { PageTitle } from './PageTitle';

const passwordSchema = z.string().min(15, '密码至少需要15个字符').max(128, '密码最多128个字符').refine((value) => new TextEncoder().encode(value).length <= 512, '密码UTF-8长度最多512字节');

export default function PasswordResetPage() {
  const navigate = useNavigate();
  const client = useQueryClient();
  const [initialToken] = useState(() => {
    const tokens = new URLSearchParams(window.location.hash.slice(1)).getAll('token');
    return tokens.length === 1 && /^[A-Za-z0-9_-]{43}$/u.test(tokens[0] ?? '') ? tokens[0] : undefined;
  });
  const token = useRef(initialToken);
  const [hasToken, setHasToken] = useState(initialToken !== undefined);
  if (window.location.hash) window.history.replaceState(null, '', window.location.pathname + window.location.search);
  const [status, setStatus] = useState<'idle' | 'success' | 'error' | 'limited' | 'unavailable'>('idle');
  const [failure, setFailure] = useState<ApiError | null>(null);
  const { register, handleSubmit, resetField, setError, formState: { errors, isSubmitting } } = useForm<{ email: string; password: string }>();
  async function submit(values: { email: string; password: string }) {
    setFailure(null); setStatus('idle');
    if (hasToken) {
      const parsed = passwordSchema.safeParse(values.password);
      if (!parsed.success) { setError('password', { message: parsed.error.issues[0]?.message ?? '请输入有效新密码' }); resetField('password', { keepError: true }); return; }
      try {
        await api.request('/auth/password-reset/confirm', passwordResetSchema, { method: 'POST', body: { token: token.current, password: parsed.data } });
        token.current = undefined; setHasToken(false); api.resetCsrf(); client.removeQueries({ queryKey: ['identity'] });
        await navigate('/login', { replace: true });
      } catch (error) { failed(error); if (error instanceof ApiError && [400, 409, 422].includes(error.status)) { token.current = undefined; setHasToken(false); } } finally { resetField('password'); }
    } else {
      const parsed = z.email('请输入有效邮箱地址').safeParse(values.email);
      if (!parsed.success) { setError('email', { message: parsed.error.issues[0]?.message ?? '请输入有效邮箱地址' }); return; }
      try { await api.request('/auth/password-reset/request', acceptedSchema, { method: 'POST', body: { email: parsed.data } }); setStatus('success'); } catch (error) { failed(error); }
    }
  }
  function failed(error: unknown) {
    const failure = error instanceof ApiError ? error : new ApiError(0, 'CLIENT_NETWORK_UNAVAILABLE', '请求暂未完成，请重试');
    setFailure(failure); setStatus(failure.status === 429 ? 'limited' : failure.status === 503 || failure.status === 0 ? 'unavailable' : 'error');
  }
  return <><PageTitle title="找回密码" /><div className="foundation-grid"><div><p className="eyebrow">ACCOUNT / PASSWORD RESET</p><h1>{hasToken ? '设置新密码' : '找回密码'}</h1><p className="lead">重置链接只有在你确认后使用。</p><p className="muted">邮箱重置不会移除 TOTP 或 Passkey，也不会自动登录。</p></div><section className="panel form-panel"><h2>{hasToken ? '重置密码' : '申请重置邮件'}</h2><form noValidate onSubmit={(event) => { void handleSubmit(submit)(event); }}>{hasToken ? <Password label="新密码" autoComplete="new-password" {...register('password')} error={errors.password?.message} /> : <Input label="邮箱地址" type="email" autoComplete="username" {...register('email')} error={errors.email?.message} />}<Button type="submit" variant="primary" className="form-submit" loading={isSubmitting} loadingLabel="正在提交…">{hasToken ? '重置密码' : '发送重置邮件'}</Button></form>{status === 'success' && <Status kind="success" title="请检查邮箱" description="如果该邮箱可接收此操作的邮件，我们将发送后续说明。" />}{failure && <Status kind={status === 'idle' || status === 'success' ? 'error' : status} title="请求暂未完成" description={failure.message} requestId={failure.requestId} />}<Link to="/login">前往登录</Link></section></div></>;
}
