import { useRef, useState } from 'react';
import { useForm } from 'react-hook-form';
import { Link } from 'react-router';
import { z } from 'zod';
import { api, acceptedSchema } from '../lib/api';
import type { ApiError } from '../lib/api';
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

const passwordSchema = z.string().refine((value) => Array.from(value).length >= 15, '密码至少需要15个字符').refine((value) => Array.from(value).length <= 128, '密码最多128个字符').refine((value) => new TextEncoder().encode(value).length <= 512, '密码长度超出限制');
const inputSchema = z.object({ email: z.email('请输入有效邮箱地址'), password: passwordSchema });
type FormInput = z.infer<typeof inputSchema>;

export default function RegisterPage() {
  const flow = useAuthTransaction();
  const retry = useRetryAfter();
  const pending = useRef(false);
  const { register, handleSubmit, setError, resetField, formState: { errors, isSubmitting } } = useForm<FormInput>();
  const [failure, setFailure] = useState<ApiError | null>(null);
  const [message, setMessage] = useState('');
  async function submit(values: FormInput) {
    if (pending.current || retry.remaining > 0 || flow.blocked) return;
    const parsed = inputSchema.safeParse(values);
    if (!parsed.success) {
      for (const issue of parsed.error.issues) { const field = issue.path[0]; if (field === 'email' || field === 'password') setError(field, { message: issue.message }); }
      resetField('password', { keepError: true }); return;
    }
    pending.current = true; setFailure(null); setMessage('');
    try {
      const response = await api.request('/auth/register', acceptedSchema, { method: 'POST', body: parsed.data });
      setMessage(response.message);
    } catch (error) { const failure = asApiError(error); setFailure(failure); retry.start(failure); }
    finally { resetField('password'); pending.current = false; }
  }
  return <><PageTitle title="创建账号" /><PublicAuthLayout eyebrow="YOUR ACCOUNT STARTS HERE" title="创建你的账号。" description="用一个账号，连接你的应用。验证邮箱后，就可以开始登录。" note={<><h2>从安全的第一步开始</h2><p>使用你可以接收邮件的地址。密码支持空格、粘贴和密码管理器。</p></>}><h2>创建账号</h2><p className="muted">请使用你的账号邮箱。</p><AuthTransactionNotice flow={flow} /><form noValidate onSubmit={(event) => { void handleSubmit(submit)(event); }}><Input label="邮箱地址" type="email" autoComplete="username" autoCapitalize="none" spellCheck={false} {...register('email')} error={errors.email?.message} /><Password label="密码" autoComplete="new-password" {...register('password')} error={errors.password?.message} aria-describedby="register-password-hint" /><p id="register-password-hint" className="field-hint password-hint">15 至 128 个字符，允许空格和粘贴。</p><Button type="submit" variant="primary" loading={isSubmitting} disabled={flow.blocked || retry.remaining > 0} loadingLabel="正在提交…" className="form-submit">创建账号</Button></form>{message && <Status kind="success" title="请检查邮箱" description={message} />}<RequestFeedback error={failure} title="申请暂未完成" remaining={retry.remaining} /><div className="auth-links"><Link to={flow.link('/email-verification')}>验证邮件或重新发送</Link><p>已有账号？<Link to={flow.link('/login')}>前往登录</Link></p></div></PublicAuthLayout></>;
}
