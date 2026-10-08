import { useRef, useState } from 'react';
import { useForm } from 'react-hook-form';
import { Link } from 'react-router';
import { z } from 'zod';
import { api, acceptedSchema, emailVerifiedSchema } from '../lib/api';
import type { ApiError } from '../lib/api';
import { asApiError, useRetryAfter } from '../lib/auth-feedback';
import { useMailAction } from '../lib/mail-action';
import { useAuthTransaction } from '../lib/auth-flow';
import { Button } from '../components/Button';
import { Input } from '../components/Input';
import { Status } from '../components/Status';
import { RequestFeedback } from '../components/RequestFeedback';
import { PublicAuthLayout } from '../components/PublicAuthLayout';
import { AuthTransactionNotice } from '../components/AuthTransactionNotice';
import { PageTitle } from './PageTitle';

export default function EmailVerificationPage() {
  const mailAction = useMailAction(30);
  const pending = useRef(false);
  const flow = useAuthTransaction();
  const retry = useRetryAfter();
  const hasToken = mailAction.hasToken;
  const [busy, setBusy] = useState(false);
  const [verified, setVerified] = useState(false);
  const [failure, setFailure] = useState<ApiError | null>(null);
  const [message, setMessage] = useState('');
  const [invalidLink, setInvalidLink] = useState(mailAction.supplied && !mailAction.hasToken);
  const { register, handleSubmit, setError, formState: { errors, isSubmitting } } = useForm<{ email: string }>();
  function failed(error: unknown) { const failure = asApiError(error); setFailure(failure); retry.start(failure); }
  async function confirm() {
    if (pending.current || retry.remaining > 0 || flow.blocked) return;
    if (mailAction.value() === undefined) { mailAction.clear(); setInvalidLink(true); return; }
    pending.current = true; setBusy(true); setFailure(null); setMessage('');
    try {
      await api.request('/auth/email-verification/confirm', emailVerifiedSchema, { method: 'POST', body: { token: mailAction.value() } });
      mailAction.clear(); setVerified(true); setMessage('邮箱已验证，请前往登录。');
    } catch (error) { failed(error); if (error instanceof Error && 'status' in error && [400, 409, 422].includes(Number(error.status))) { mailAction.clear(); } }
    finally { pending.current = false; setBusy(false); }
  }
  async function resend({ email }: { email: string }) {
    if (pending.current || retry.remaining > 0 || flow.blocked) return;
    const parsed = z.email('请输入有效邮箱地址').safeParse(email);
    if (!parsed.success) { setError('email', { message: '请输入有效邮箱地址' }); return; }
    pending.current = true; setFailure(null); setMessage('');
    try { const result = await api.request('/auth/email-verification/request', acceptedSchema, { method: 'POST', body: { email: parsed.data } }); setMessage(result.message); }
    catch (error) { failed(error); } finally { pending.current = false; }
  }
  return <><PageTitle title="验证邮箱" /><PublicAuthLayout eyebrow="CONFIRM YOUR EMAIL" title="确认你的邮箱。" description="确认这是你的邮箱后，就可以使用账号登录。" note={<><h2>由你确认，再继续</h2><p>邮件链接不会自动完成验证。打开链接后请点击确认；刷新或离开页面后，需要重新打开邮件中的链接。</p></>}><h2>验证邮箱</h2><AuthTransactionNotice flow={flow} />{hasToken && <><p>请确认验证当前邮件中的邮箱地址。</p><Button className="form-submit" variant="primary" loading={busy} disabled={retry.remaining > 0 || flow.blocked || isSubmitting} loadingLabel="正在验证…" onClick={() => { void confirm(); }}>确认验证邮箱</Button></>}{invalidLink && <Status kind="error" title="验证链接无效或已过期" description="请重新打开邮件链接，或申请新的验证邮件。" />}{message && <Status kind="success" title={verified ? '邮箱验证完成' : '请检查邮箱'} description={message} />}<RequestFeedback error={failure} title="验证暂未完成" remaining={retry.remaining} />{verified ? <Link className="button button--primary form-submit" to={flow.link('/login')}>前往登录</Link> : <form noValidate onSubmit={(event) => { void handleSubmit(resend)(event); }}><p className="muted">没有收到邮件？重新申请后请使用最新的验证链接。</p><Input label="邮箱地址" type="email" autoComplete="username" autoCapitalize="none" spellCheck={false} {...register('email')} error={errors.email?.message} /><Button type="submit" className="form-submit" disabled={busy || retry.remaining > 0 || flow.blocked} loading={isSubmitting} loadingLabel="正在提交…">重新发送验证邮件</Button></form>}<div className="auth-links"><Link to={flow.link('/register')}>创建账号</Link><Link to={flow.link('/login')}>返回登录</Link></div></PublicAuthLayout></>;
}
