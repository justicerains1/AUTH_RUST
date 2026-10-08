import { useRef, useState } from 'react';
import { useForm } from 'react-hook-form';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate } from 'react-router';
import { z } from 'zod';
import { api, ApiError, meSchema, passwordLoginSchema } from '../lib/api';
import type { PasswordLogin } from '../lib/api';
import { asApiError, useRetryAfter } from '../lib/auth-feedback';
import { useAuthTransaction } from '../lib/auth-flow';
import { Button } from '../components/Button';
import { Input } from '../components/Input';
import { Password } from '../components/Password';
import { Status } from '../components/Status';
import { RequestFeedback } from '../components/RequestFeedback';
import { PublicAuthLayout } from '../components/PublicAuthLayout';
import { AuthTransactionNotice } from '../components/AuthTransactionNotice';
import { PageTitle } from './PageTitle';
import { FactorChallenge } from '../components/FactorChallenge';
import { PasskeyLoginButton } from '../components/PasskeyLoginButton';

const inputSchema = z.object({ email: z.email('请输入有效邮箱地址'), password: z.string().min(1, '请输入密码').refine((value) => Array.from(value).length <= 128 && new TextEncoder().encode(value).length <= 512, '密码长度超出限制') });
type LoginInput = z.infer<typeof inputSchema>;
type MfaChallenge = Extract<PasswordLogin, { status: 'mfa_required' }>;

export default function LoginPage() {
  const client = useQueryClient();
  const navigate = useNavigate();
  const flow = useAuthTransaction();
  const retry = useRetryAfter();
  const pending = useRef(false);
  const passwordInput = useRef<HTMLInputElement | null>(null);
  const { register, handleSubmit, setError, resetField, formState: { errors, isSubmitting } } = useForm<LoginInput>();
  const [passkeyBusy, setPasskeyBusy] = useState(false);
  const [failure, setFailure] = useState<ApiError | null>(null);
  const [challenge, setChallenge] = useState<MfaChallenge | null>(null);
  const current = useQuery({ queryKey: ['identity', 'me'], retry: false, queryFn: ({ signal }) => api.request('/me', meSchema, { signal }) });
  const alreadyAuthenticated = current.isSuccess && !current.isFetching;
  async function authenticated(result: Extract<PasswordLogin, { status: 'authenticated' }>) {
    api.acceptRotatedCsrf(result.csrf_token);
    client.removeQueries({ queryKey: ['identity'] });
    await navigate(flow.destination, { replace: true });
  }
  async function submit(values: LoginInput) {
    if (pending.current || passkeyBusy || retry.remaining > 0 || flow.blocked) return;
    const parsed = inputSchema.safeParse(values);
    if (!parsed.success) {
      for (const issue of parsed.error.issues) { const field = issue.path[0]; if (field === 'email' || field === 'password') setError(field, { message: issue.message }); }
      resetField('password', { keepError: true }); return;
    }
    pending.current = true; setFailure(null); setChallenge(null);
    try {
      const response = await api.request('/auth/login/password', passwordLoginSchema, { method: 'POST', body: parsed.data });
      if (response.status === 'mfa_required') { api.resetCsrf(); client.removeQueries({ queryKey: ['identity', 'me'] }); setChallenge(response); return; }
      await authenticated(response);
    } catch (error) { const failure = asApiError(error); setFailure(failure); retry.start(failure); }
    finally { resetField('password'); pending.current = false; }
  }
  const passwordProps = register('password');
  return <><PageTitle title="登录" /><PublicAuthLayout eyebrow="WELCOME BACK" title="登录" description="欢迎回来。从这里继续连接你的应用，或管理账号安全。" note={<><h2>你的账号，由你掌控</h2><p>登录后可查看活跃会话和授权应用，开启双重验证，为重要操作增加一道保护。</p></>}><h2>登录账号</h2><p className="muted">请使用已验证的账号邮箱。</p><AuthTransactionNotice flow={flow} />{alreadyAuthenticated ? <><Status kind="success" title="你已登录" description="可以继续前往账号或应用。" /><Link className="button button--primary form-submit" to={flow.destination}>继续</Link></> : challenge ? <><Status kind="loading" title="还需要完成第二因素验证" description="密码已验证，此时尚未登录。请继续验证你的身份。" /><FactorChallenge challenge={challenge} purpose="login" onComplete={async (result) => { if (result.status !== 'authenticated') throw new ApiError(0, 'CLIENT_INVALID_RESPONSE', '认证结果不适用于登录'); await authenticated(result); }} onRestart={() => { setChallenge(null); resetField('password'); }} /></> : <><form noValidate onSubmit={(event) => { void handleSubmit(submit)(event); }}><Input label="邮箱地址" type="email" autoComplete="username" autoCapitalize="none" spellCheck={false} {...register('email')} error={errors.email?.message} /><Password label="密码" autoComplete="current-password" {...passwordProps} ref={(element) => { passwordProps.ref(element); passwordInput.current = element; }} error={errors.password?.message} /><p className="forgot-password"><Link to={flow.link('/password-reset')}>忘记密码？</Link></p><Button className="form-submit" type="submit" variant="primary" loading={isSubmitting} disabled={passkeyBusy || retry.remaining > 0 || flow.blocked} loadingLabel="正在登录…">登录</Button></form><p className="auth-divider">或使用通行密钥</p><PasskeyLoginButton disabled={isSubmitting || flow.blocked || retry.remaining > 0} onBusyChange={setPasskeyBusy} onAuthenticated={authenticated} onPasswordFallback={() => { passwordInput.current?.focus(); }} /></>}<RequestFeedback error={failure} title="登录未完成" remaining={retry.remaining} /><div className="auth-links"><p>尚未创建账号？<Link to={flow.link('/register')}>创建账号</Link></p><Link to={flow.link('/email-verification')}>验证邮箱或重新发送邮件</Link></div></PublicAuthLayout></>;
}
