import { useState } from 'react';
import { useForm } from 'react-hook-form';
import { useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate } from 'react-router';
import { z } from 'zod';
import { api, ApiError, passwordLoginSchema } from '../lib/api';
import type { PasswordLogin } from '../lib/api';
import { Button } from '../components/Button';
import { Input } from '../components/Input';
import { Password } from '../components/Password';
import { Status } from '../components/Status';
import { PageTitle } from './PageTitle';
import { FactorChallenge } from '../components/FactorChallenge';

const inputSchema = z.object({ email: z.email('请输入有效邮箱地址'), password: z.string().min(1, '请输入密码').max(128, '密码最多128个字符') });
type LoginInput = z.infer<typeof inputSchema>;
type MfaChallenge = Extract<PasswordLogin, { status: 'mfa_required' }>;

export default function LoginPage() {
  const client = useQueryClient();
  const navigate = useNavigate();
  const { register, handleSubmit, setError, resetField, formState: { errors, isSubmitting } } = useForm<LoginInput>();
  const [failure, setFailure] = useState<ApiError | null>(null);
  const [challenge, setChallenge] = useState<MfaChallenge | null>(null);
  async function submit(values: LoginInput) {
    const parsed = inputSchema.safeParse(values);
    if (!parsed.success) {
      for (const issue of parsed.error.issues) { const field = issue.path[0]; if (field === 'email' || field === 'password') setError(field, { message: issue.message }); }
      resetField('password', { keepError: true });
      return;
    }
    setFailure(null);
    setChallenge(null);
    try {
      const response = await api.request('/auth/login/password', passwordLoginSchema, { method: 'POST', body: parsed.data });
      client.removeQueries({ queryKey: ['identity'] });
      if (response.status === 'mfa_required') { api.resetCsrf(); setChallenge(response); return; }
      api.acceptRotatedCsrf(response.csrf_token);
      await navigate('/me', { replace: true });
    } catch (error) { setFailure(error instanceof ApiError ? error : new ApiError(0, 'CLIENT_NETWORK_UNAVAILABLE', '请求暂未完成，请重试')); } finally { resetField('password'); }
  }
  return <><PageTitle title="登录" /><div className="foundation-grid"><div><p className="eyebrow">ACCOUNT ACCESS</p><h1>登录</h1><p className="lead">从统一入口，进入你的账号。</p><p className="muted">使用已验证邮箱和密码登录。</p></div><section className="panel form-panel"><h2>密码登录</h2>{challenge ? <><Status kind="limited" title="还需要完成第二因素验证" description="密码已验证，此时尚未登录。请继续完成第二因素验证。" /><FactorChallenge challenge={challenge} purpose="login" onComplete={async (result) => { if (result.status !== 'authenticated') throw new ApiError(0, 'CLIENT_INVALID_RESPONSE', '认证结果不适用于登录'); api.acceptRotatedCsrf(result.csrf_token); client.removeQueries({ queryKey: ['identity'] }); await navigate('/me', { replace: true }); }} onRestart={() => { setChallenge(null); }} /></> : <form noValidate onSubmit={(event) => { void handleSubmit(submit)(event); }}><Input label="邮箱地址" type="email" autoComplete="username" {...register('email')} error={errors.email?.message} /><Password label="密码" autoComplete="current-password" {...register('password')} error={errors.password?.message} /><Button className="form-submit" type="submit" variant="primary" loading={isSubmitting} loadingLabel="正在登录…">登录</Button></form>}{failure && <Status kind={failure.status === 429 ? 'limited' : failure.status === 503 || failure.status === 0 ? 'unavailable' : 'error'} title="登录未完成" description={failure.retryAfter === undefined ? failure.message : `${failure.message}，请在${String(failure.retryAfter)}秒后重试。`} requestId={failure.requestId} />}<p><Link to="/password-reset">忘记密码？</Link></p><p>尚未创建账号？<Link to="/register">创建账号</Link></p><p><Link to="/email-verification">验证邮箱或重新发送邮件</Link></p></section></div></>;
}
