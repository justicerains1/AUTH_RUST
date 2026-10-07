import { useState } from 'react';
import { useForm } from 'react-hook-form';
import { z } from 'zod';
import { Button } from '../components/Button';
import { Input } from '../components/Input';
import { Password } from '../components/Password';
import { Status } from '../components/Status';
import { Empty } from '../components/Empty';
import { Table } from '../components/Table';
import { CursorPagination } from '../components/CursorPagination';
import { ConfirmDialog } from '../components/ConfirmDialog';
import { PageTitle } from './PageTitle';

const formSchema = z.object({ email: z.email('请输入有效邮箱地址'), password: z.string().min(15, '请输入至少 15 个字符'), code: z.string().regex(/^\d{6}$/u, '请输入 6 位数字验证码') });
type Form = z.infer<typeof formSchema>;
const records = [{ id: 'first', name: '示例记录 A', status: '组件展示' }, { id: 'second', name: '示例记录 B', status: '组件展示' }];

export default function ComponentsPage() {
  const [formStatus, setFormStatus] = useState<'idle' | 'valid'>('idle');
  const [page, setPage] = useState(0);
  const [confirmed, setConfirmed] = useState(false);
  const { register, handleSubmit, setError, resetField, formState: { errors } } = useForm<Form>({ defaultValues: { email: '', password: '', code: '' } });
  function validate(values: Form) {
    const result = formSchema.safeParse(values);
    if (!result.success) {
      for (const issue of result.error.issues) {
        const field = issue.path[0];
        if (field === 'email' || field === 'password' || field === 'code') setError(field, { type: 'validation', message: issue.message });
      }
      resetField('password', { keepError: true });
      resetField('code', { keepError: true });
      setFormStatus('idle');
      return;
    }
    resetField('password');
    resetField('code');
    setFormStatus('valid');
  }
  return <><PageTitle title="组件与状态" /><p className="eyebrow">DESIGN SYSTEM / T16</p><h1>组件与反馈状态</h1><p className="lead">基础控件的键盘、表单和反馈行为。</p><p className="muted">开发示例仅检查控件；不执行登录或修改账号。</p><div className="component-grid">
    <section className="panel"><h2>按钮</h2><div className="actions"><Button variant="primary" onClick={() => { setFormStatus('idle'); }}>继续检查</Button><Button disabled>暂不可用</Button><Button loading loadingLabel="正在验证…">验证</Button></div><p className="muted">加载时保留文字，禁止重复提交。</p></section>
    <section className="panel"><h2>表单与错误</h2><form noValidate onSubmit={(event) => { void handleSubmit(validate)(event); }}><Input label="邮箱地址" type="email" autoComplete="username" {...register('email')} error={errors.email?.message} /><Password label="密码" autoComplete="current-password" {...register('password')} error={errors.password?.message} /><Input label="验证码" inputMode="numeric" autoComplete="one-time-code" {...register('code')} error={errors.code?.message} /><Button className="form-submit" type="submit" variant="primary">检查输入</Button></form>{formStatus === 'valid' && <Status kind="success" title="输入格式符合示例规则" description="这只是本地控件检查，没有提交认证请求。" />}</section>
    <section className="panel"><h2>状态反馈</h2><div className="stack"><Status kind="loading" title="正在加载…" /><Status kind="error" title="请求无法完成" description="请检查输入后重新尝试。" /><Status kind="limited" title="请求较频繁" description="请在服务端指定的等待时间后再试。" /><Status kind="unavailable" title="暂时无法加载" description="服务暂不可用。" onRetry={() => { setFormStatus('idle'); }} /></div></section>
    <section className="panel"><h2>空状态</h2><Empty title="暂无记录" description="记录产生后，你可以在这里查看。" /></section>
    <section className="panel"><h2>确认对话框</h2><p>初始焦点在取消，关闭后回到触发按钮。</p><ConfirmDialog trigger={<Button>打开确认对话框</Button>} title="确认示例操作？" description="这个操作只关闭开发示例对话框，不修改任何账号。" confirmLabel="完成示例" onConfirm={() => { setConfirmed(true); }} />{confirmed && <Status kind="success" title="示例对话框已确认" />}</section>
    <section className="panel"><h2>表格与分页</h2><Table caption="示例记录" columns={[{ key: 'name', title: '记录名称', render: (row) => row.name }, { key: 'status', title: '状态', render: (row) => row.status }]} rows={records.slice(page, page + 1)} rowKey={(row) => row.id} /><CursorPagination hasPrevious={page > 0} hasNext={page < records.length - 1} onPrevious={() => { setPage((value) => value - 1); }} onNext={() => { setPage((value) => value + 1); }} /></section>
  </div></>;
}
