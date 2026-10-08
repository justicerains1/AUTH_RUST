import { useRef, useState } from 'react';
import { useForm } from 'react-hook-form';
import { useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router';
import { z } from 'zod';
import { api, acceptedSchema, passwordResetSchema } from '../lib/api';
import type { ApiError } from '../lib/api';
import { asApiError, useRetryAfter } from '../lib/auth-feedback';
import { useAuthTransaction } from '../lib/auth-flow';
import { useMailAction } from '../lib/mail-action';
import { Button } from '../components/Button';
import { Input } from '../components/Input';
import { Password } from '../components/Password';
import { Status } from '../components/Status';
import { RequestFeedback } from '../components/RequestFeedback';
import { PublicAuthLayout } from '../components/PublicAuthLayout';
import { AuthTransactionNotice } from '../components/AuthTransactionNotice';
import { PageTitle } from './PageTitle';

const passwordSchema = z.string().refine((value) => Array.from(value).length >= 15, '密码至少需要15个字符').refine((value) => Array.from(value).length <= 128, '密码最多128个字符').refine((value) => new TextEncoder().encode(value).length <= 512, '密码长度超出限制');

export default function PasswordResetPage() {
  const client = useQueryClient();
  const flow = useAuthTransaction();
  const retry = useRetryAfter();
  const mailAction = useMailAction(15);
  const pending = useRef(false);
  const hasToken = mailAction.hasToken;
  const [completed, setCompleted] = useState(false);
  const [invalidLink, setInvalidLink] = useState(mailAction.supplied && !mailAction.hasToken);
  const [message, setMessage] = useState('');
  const [failure, setFailure] = useState<ApiError | null>(null);
  const { register, handleSubmit, resetField, setError, formState: { errors, isSubmitting } } = useForm<{ email: string; password: string }>();
  function failed(error: unknown) { const failure = asApiError(error); setFailure(failure); retry.start(failure); }
  async function submit(values: { email: string; password: string }) {
    if (pending.current || retry.remaining > 0 || flow.blocked) return;
    setFailure(null); setMessage('');
    if (hasToken) {
      if (mailAction.value() === undefined) { mailAction.clear(); setInvalidLink(true); resetField('password'); return; }
      const parsed = passwordSchema.safeParse(values.password);
      if (!parsed.success) { setError('password', { message: parsed.error.issues[0]?.message ?? '请输入有效新密码' }); resetField('password', { keepError: true }); return; }
      pending.current = true;
      try {
        await api.request('/auth/password-reset/confirm', passwordResetSchema, { method: 'POST', body: { token: mailAction.value(), password: parsed.data } });
        mailAction.clear(); setCompleted(true); api.resetCsrf(); client.removeQueries({ queryKey: ['identity', 'me'] });
      } catch (error) { failed(error); if (error instanceof Error && 'status' in error && [400, 409, 422].includes(Number(error.status))) { mailAction.clear(); } }
      finally { resetField('password'); pending.current = false; }
    } else {
      const parsed = z.email('请输入有效邮箱地址').safeParse(values.email);
      if (!parsed.success) { setError('email', { message: '请输入有效邮箱地址' }); return; }
      pending.current = true;
      try { const result = await api.request('/auth/password-reset/request', acceptedSchema, { method: 'POST', body: { email: parsed.data } }); setMessage(result.message); }
      catch (error) { failed(error); } finally { pending.current = false; }
    }
  }
  return <><PageTitle title={hasToken ? '设置新密码' : '找回密码'} /><PublicAuthLayout eyebrow="RETURN TO YOUR ACCOUNT" title={hasToken ? '设置新密码' : '找回密码'} description={hasToken ? '选择一个新密码，然后确认重置。' : '输入你的账号邮箱，我们将发送后续说明。'} note={<><h2>保留你的账号保护</h2><p>重置密码不会移除双重验证或通行密钥。完成后需要重新登录。</p></>}><h2>{completed ? '密码重置完成' : hasToken ? '重置密码' : '申请重置邮件'}</h2><AuthTransactionNotice flow={flow} />{completed ? <><Status kind="success" title="新密码已设置" description="请使用新密码登录；已开启的第二因素仍需验证。" /><Link className="button button--primary form-submit" to={flow.link('/login')}>前往登录</Link></> : <>{invalidLink && <Status kind="error" title="重置链接无效或已过期" description="请重新打开邮件中的链接，或申请新的重置邮件。" />}<p className="muted">{hasToken ? '只有点击“重置密码”才会使用邮件链接。' : '如果没有收到邮件，请检查垃圾邮件或重新申请。'}</p><form noValidate onSubmit={(event) => { void handleSubmit(submit)(event); }}>{hasToken ? <><Password label="新密码" autoComplete="new-password" {...register('password')} error={errors.password?.message} aria-describedby="reset-password-hint" /><p id="reset-password-hint" className="field-hint password-hint">15 至 128 个字符，允许空格和粘贴。</p></> : <Input label="邮箱地址" type="email" autoComplete="username" autoCapitalize="none" spellCheck={false} {...register('email')} error={errors.email?.message} />}<Button type="submit" variant="primary" className="form-submit" disabled={retry.remaining > 0 || flow.blocked} loading={isSubmitting} loadingLabel="正在提交…">{hasToken ? '重置密码' : '发送重置邮件'}</Button></form>{message && <Status kind="success" title="请检查邮箱" description={message} />}<RequestFeedback error={failure} title="请求暂未完成" remaining={retry.remaining} /><div className="auth-links"><Link to={flow.link('/login')}>前往登录</Link></div></>}</PublicAuthLayout></>;
}
