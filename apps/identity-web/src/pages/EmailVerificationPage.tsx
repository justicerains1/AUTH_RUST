import { useRef, useState } from 'react';
import { useForm } from 'react-hook-form';
import { Link } from 'react-router';
import { api, acceptedSchema, emailVerifiedSchema, ApiError } from '../lib/api';
import { Button } from '../components/Button';
import { Input } from '../components/Input';
import { Status } from '../components/Status';
import { PageTitle } from './PageTitle';

export default function EmailVerificationPage() {
  const [initialToken] = useState(() => {
    if (!window.location.hash) return undefined;
    const values = new URLSearchParams(window.location.hash.slice(1));
    const tokens = values.getAll('token');
    return tokens.length === 1 && /^[A-Za-z0-9_-]{43}$/u.test(tokens[0] ?? '') ? tokens[0] : undefined;
  });
  const token = useRef(initialToken); const [hasToken, setHasToken] = useState(initialToken !== undefined);
  // Remove fragment before displaying UI or sending any request; never persist its secret.
  if (window.location.hash) window.history.replaceState(null, '', window.location.pathname + window.location.search);
  const [busy, setBusy] = useState(false); const [verified, setVerified] = useState(false);
  const [status, setStatus] = useState<'idle' | 'success' | 'error' | 'limited' | 'unavailable'>('idle');
  const [message, setMessage] = useState(''); const [requestId, setRequestId] = useState<string>();
  const { register, handleSubmit, formState: { errors, isSubmitting } } = useForm<{ email: string }>();
  function failed(error: unknown) {
    setStatus(error instanceof ApiError && error.status === 429 ? 'limited' : error instanceof ApiError && (error.status === 503 || error.status === 0) ? 'unavailable' : 'error');
    setMessage(error instanceof ApiError ? error.message : '请求暂未完成，请重试');
    setRequestId(error instanceof ApiError ? error.requestId : undefined);
  }
  async function confirm() {
    if (!token.current || busy) return;
    setBusy(true); setStatus('idle');
    try {
      await api.request('/auth/email-verification/confirm', emailVerifiedSchema, { method: 'POST', body: { token: token.current } });
      token.current = undefined; setHasToken(false); setVerified(true); setStatus('success'); setMessage('邮箱已验证，请前往登录。');
    } catch (error) {
      failed(error);
      if (error instanceof ApiError && [400, 409, 422].includes(error.status)) { token.current = undefined; setHasToken(false); }
    } finally { setBusy(false); }
  }
  return <><PageTitle title="验证邮箱" /><div className="foundation-grid"><div><p className="eyebrow">ACCOUNT / VERIFICATION</p><h1>确认你的邮箱。</h1><p className="lead">验证链接只会在你点击确认后使用。</p></div><section className="panel form-panel"><h2>验证邮箱</h2>
    {hasToken && <Button variant="primary" loading={busy} loadingLabel="正在验证…" onClick={() => { void confirm(); }}>确认验证邮箱</Button>}
    {status !== 'idle' && <Status kind={status} title={verified ? '邮箱验证完成' : status === 'success' ? '请检查邮箱' : '验证暂未完成'} description={message} requestId={requestId} />}
    {verified ? <Link className="button button--primary" to="/login">前往登录</Link> : <form noValidate onSubmit={(event) => { void handleSubmit(async ({ email }) => {
      setStatus('idle');
      try { const result = await api.request('/auth/email-verification/request', acceptedSchema, { method: 'POST', body: { email } }); setStatus('success'); setMessage(result.message); }
      catch (error) { failed(error); }
    })(event); }}><Input label="邮箱地址" type="email" autoComplete="username" {...register('email', { required: '请输入邮箱地址', pattern: { value: /^[^\s@]+@[^\s@]+\.[^\s@]+$/u, message: '请输入有效邮箱地址' } })} error={errors.email?.message} /><Button type="submit" loading={isSubmitting} loadingLabel="正在提交…">重新发送验证邮件</Button></form>}
    <p><Link to="/register">创建账号</Link></p></section></div></>;
}
