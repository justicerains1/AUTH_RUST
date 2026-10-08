import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from 'react-router';
import { api, noContentSchema, sessionPageSchema } from '../lib/api';
import { Button } from '../components/Button';
import { AccountAction } from '../components/AccountAction';
import { AccountLayout } from '../components/AccountLayout';
import { CursorPagination } from '../components/CursorPagination';
import { Table } from '../components/Table';

export default function SessionsPage() {
  const navigate = useNavigate();
  const client = useQueryClient();
  const [cursors, setCursors] = useState<string[]>([]);
  const cursor = cursors.at(-1);
  const query = useQuery({ queryKey: ['identity', 'sessions', cursor ?? ''], queryFn: ({ signal }) => api.request(`/me/sessions?limit=20${cursor === undefined ? '' : `&cursor=${encodeURIComponent(cursor)}`}`, sessionPageSchema, { signal }) });
  async function logout() { api.resetCsrf(); client.removeQueries({ queryKey: ['identity'] }); await navigate('/login', { replace: true }); }
  async function revoke(id: string, current: boolean) {
    await api.request(`/me/sessions/${id}`, noContentSchema, { method: 'DELETE' });
    if (current) { await logout(); return; }
    await client.invalidateQueries({ queryKey: ['identity', 'sessions'] });
  }
  async function revokeAll() { await api.request('/auth/logout-all', noContentSchema, { method: 'POST' }); await logout(); }
  return <AccountLayout title="设备会话" description="查看本账号的设备；撤销后，该设备下一次认证检查将失效。"><section className="panel"><div className="security-header"><div><h2>活跃设备</h2><p className="muted">当前设备会明确标记。撤销其他设备不会使当前设备退出。</p></div><AccountAction trigger={<Button variant="danger">退出所有设备</Button>} title="退出所有设备？" description="所有设备会话和由它们派生的应用授权将失效，你也需要重新登录。" confirmLabel="退出所有设备" onConfirm={revokeAll} /></div><Table caption="设备会话列表" columns={[{ key: 'device', title: '设备', render: (row) => <><strong>{row.user_agent || '未识别设备'}</strong>{row.current && <span className="current-device">当前设备</span>}</> }, { key: 'created', title: '登录时间', render: (row) => <span className="date-value">{new Date(row.auth_time).toLocaleString('zh-CN')}</span> }, { key: 'action', title: '操作', render: (row) => <AccountAction trigger={<Button variant="danger">{row.current ? '退出当前设备' : '撤销会话'}</Button>} title={row.current ? '退出当前设备？' : '撤销这次会话？'} description={row.current ? '当前设备会话和相关应用授权将失效，你需要重新登录。' : '该设备及相关应用授权将失效，其他设备不受影响。'} confirmLabel="确认撤销" onConfirm={() => revoke(row.id, row.current)} /> }]} rows={query.data?.items ?? []} rowKey={(row) => row.id} loading={query.isPending} unavailable={query.isError} onRetry={() => { void query.refetch(); }} emptyTitle="没有活动设备会话" emptyDescription="活动会话将显示在此处。" /><CursorPagination hasPrevious={cursors.length > 0} hasNext={Boolean(query.data?.next_cursor)} loading={query.isFetching} onPrevious={() => { setCursors((values) => values.slice(0, -1)); }} onNext={() => { const next = query.data?.next_cursor; if (next) setCursors((values) => [...values, next]); }} /></section></AccountLayout>;
}
