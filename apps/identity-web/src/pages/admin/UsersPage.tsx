import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { adminApi } from '../../lib/admin-api';
import { Input } from '../../components/Input';
import { Button } from '../../components/Button';
import { Status } from '../../components/Status';
import { CursorPagination } from '../../components/CursorPagination';
import { Empty } from '../../components/Empty';
import { AdminAction, AdminFailure, AdminPanel, AdminTable } from './AdminShared';
import { PageTitle } from '../PageTitle';

export default function UsersPage() {
  const [email, setEmail] = useState(''); const [status, setStatus] = useState<'' | 'active' | 'disabled'>(''); const [filter, setFilter] = useState<{ email?: string; status?: 'active' | 'disabled' }>({}); const [cursor, setCursor] = useState<string | undefined>(); const [history, setHistory] = useState<(string | undefined)[]>([]); const [selected, setSelected] = useState<string | null>(null);
  const query = useQuery({ queryKey: ['admin', 'users', filter, cursor], queryFn: ({ signal }) => adminApi.users({ ...filter, cursor }, signal), retry: false });
  const detail = useQuery({ queryKey: ['admin', 'user', selected], queryFn: ({ signal }) => adminApi.user(selected ?? '', signal), enabled: selected !== null, retry: false });
  return <><PageTitle title="用户管理" /><h1>用户管理</h1><p className="muted">按完整邮箱或账号状态查询，安全变化由服务器再次验证。</p><AdminPanel><form className="admin-filters" onSubmit={(event) => { event.preventDefault(); setFilter({ ...(email.trim() === '' ? {} : { email: email.trim() }), ...(status === '' ? {} : { status }) }); setCursor(undefined); setHistory([]); setSelected(null); }}><Input label="完整邮箱" type="email" value={email} onChange={(event) => { setEmail(event.target.value); }} /><label>账号状态<select value={status} onChange={(event) => { setStatus(event.target.value as '' | 'active' | 'disabled'); }}><option value="">全部状态</option><option value="active">可用</option><option value="disabled">已禁用</option></select></label><Button type="submit">查询用户</Button></form>
      {query.isPending ? <Status kind="loading" title="正在查询用户…" /> : query.isError ? <AdminFailure error={query.error} retry={() => { void query.refetch(); }} /> : query.data.items.length === 0 ? <Empty title="没有符合条件的用户" description="可以修改完整邮箱或账号状态后查询。" /> : <AdminTable caption="用户列表" columns={[{ key: 'email', header: '邮箱' }, { key: 'status', header: '状态' }, { key: 'verified', header: '邮箱验证' }, { key: 'action', header: '操作' }]} rows={query.data.items.map((user) => ({ id: user.id, cells: { email: user.email, status: user.status === 'active' ? '可用' : '已禁用', verified: user.email_verified ? '已验证' : '未验证', action: <Button onClick={() => { setSelected(user.id); }}>查看用户</Button> } }))} />}
      <CursorPagination hasPrevious={history.length > 0} hasNext={query.data?.next_cursor != null} loading={query.isFetching} onPrevious={() => { setCursor(history.at(-1)); setHistory(history.slice(0, -1)); }} onNext={() => { const next = query.data?.next_cursor; if (next) { setHistory([...history, cursor]); setCursor(next); } }} />
    </AdminPanel>{selected !== null && <AdminPanel><h2>用户详情</h2>{detail.isPending ? <Status kind="loading" title="正在读取用户…" /> : detail.isError ? <AdminFailure error={detail.error} /> : <><p>{detail.data.email}</p><p>创建时间：{new Date(detail.data.created_at).toLocaleString('zh-CN')}</p><div className="actions"><AdminAction label={detail.data.status === 'active' ? '禁用账号' : '启用账号'} title="更改账号状态" description="禁用将使账号的现有会话和应用授权失效；最后可用管理员受到保护。" danger={detail.data.status === 'active'} action={() => adminApi.setStatus(selected, detail.data.status === 'active' ? 'disabled' : 'active')} onComplete={async () => { await query.refetch(); await detail.refetch(); }} /><AdminAction label="撤销全部会话" title="撤销此账号的全部会话？" description="此用户所有设备及派生应用授权将失效，无法取消正在执行的业务请求。" danger action={() => adminApi.revokeSessions(selected)} onComplete={async () => { await detail.refetch(); }} /></div></>}</AdminPanel>}</>;
}
