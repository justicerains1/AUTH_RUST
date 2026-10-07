import { useState } from 'react';
import { useForm } from 'react-hook-form';
import { Link } from 'react-router';
import { z } from 'zod';
import { api, acceptedSchema, ApiError } from '../lib/api';
import { Button } from '../components/Button';
import { Input } from '../components/Input';
import { Password } from '../components/Password';
import { Status } from '../components/Status';
import { PageTitle } from './PageTitle';

const inputSchema = z.object({ email: z.email('请输入有效邮箱地址'), password: z.string().min(15, '密码至少需要15个字符').max(128, '密码最多128个字符') });
type FormInput = z.infer<typeof inputSchema>;

export default function RegisterPage() {
  const { register, handleSubmit, setError, resetField, formState: { errors, isSubmitting } } = useForm<FormInput>();
  const [result, setResult] = useState<'idle' | 'success' | 'error' | 'limited' | 'unavailable'>('idle');
  const [message, setMessage] = useState(''); const [requestId, setRequestId] = useState<string>();
  async function submit(values: FormInput) {
    const parsed = inputSchema.safeParse(values);
    if (!parsed.success) {
      for (const issue of parsed.error.issues) { const field = issue.path[0]; if (field === 'email' || field === 'password') setError(field, { message: issue.message }); }
      resetField('password', { keepError: true }); return;
    }
    setResult('idle'); setRequestId(undefined);
    try {
      const response = await api.request('/auth/register', acceptedSchema, { method: 'POST', body: parsed.data });
      setResult('success'); setMessage(response.message);
    } catch (error) {
      setResult(error instanceof ApiError && error.status === 429 ? 'limited' : error instanceof ApiError && (error.status === 503 || error.status === 0) ? 'unavailable' : 'error');
      setMessage(error instanceof ApiError ? error.message : '请求暂未完成，请重试');
      if (error instanceof ApiError) setRequestId(error.requestId);
    } finally { resetField('password'); }
  }
  return <><PageTitle title="创建账号" /><div className="foundation-grid"><div><p className="eyebrow">ACCOUNT / REGISTER</p><h1>创建你的账号。</h1><p className="lead">通过邮箱验证，开始使用统一身份中心。</p></div><section className="panel form-panel"><h2>创建账号</h2><form noValidate onSubmit={(event) => { void handleSubmit(submit)(event); }}>
    <Input label="邮箱地址" type="email" autoComplete="username" {...register('email')} error={errors.email?.message} />
    <Password label="密码" autoComplete="new-password" {...register('password')} error={errors.password?.message} />
    <p className="muted">15至128个字符，允许空格和粘贴。</p>
    <Button type="submit" variant="primary" loading={isSubmitting} loadingLabel="正在提交…" className="form-submit">创建账号</Button></form>
    {result !== 'idle' && <Status kind={result} title={result === 'success' ? '请检查邮箱' : '申请暂未完成'} description={message} requestId={requestId} />}
    <p><Link to="/email-verification">验证邮件或重新发送</Link></p><p>已有账号？<Link to="/login">前往登录</Link></p></section></div></>;
}
