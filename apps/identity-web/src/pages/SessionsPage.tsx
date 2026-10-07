import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate } from 'react-router';
import { api, ApiError, noContentSchema, sessionPageSchema } from '../lib/api';
import { Button } from '../components/Button';
import { ConfirmDialog } from '../components/ConfirmDialog';
import { CursorPagination } from '../components/CursorPagination';
import { Status } from '../components/Status';
import { Table } from '../components/Table';
import { PageTitle } from './PageTitle';

export default function SessionsPage() {
  const navigate = useNavigate();
  const client = useQueryClient();
  const [cursors, setCursors] = useState<string[]>([]);
  const [failure, setFailure] = useState<ApiError | null>(null);
  const cursor = cursors.at(-1);
  const query = useQuery({ queryKey: ['identity', 'sessions', cursor ?? ''], queryFn: ({ signal }) => api.request(`/me/sessions?limit=20${cursor === undefined ? '' : `&cursor=${encodeURIComponent(cursor)}`}`, sessionPageSchema, { signal }) });
  async function revoke(id: string, current: boolean) {
    try {
      await api.request(`/me/sessions/${id}`, noContentSchema, { method: 'DELETE' });
      setFailure(null);
      if (current) { api.resetCsrf(); client.removeQueries({ queryKey: ['identity'] }); await navigate('/login', { replace: true }); return; }
      await client.invalidateQueries({ queryKey: ['identity', 'sessions'] });
    } catch (error) { setFailure(error instanceof ApiError ? error : new ApiError(0, 'CLIENT_NETWORK_UNAVAILABLE', '操作暂未完成')); throw error; }
  }
  return <><PageTitle title="设备会话" /><p className="eyebrow">ACCOUNT / SESSIONS</p><h1>设备会话</h1><p className="muted">查看本账号的设备；撤销后，该设备下一次认证检查将失效。</p><p><Link to="/me">返回账号安全</Link></p>{failure && <Status kind="error" title="撤销未完成" description={failure.message} requestId={failure.requestId} />}<section className="panel"><Table caption="设备会话列表" columns={[{ key: 'device', title: '设备', render: (row) => <>{row.user_agent || '未识别设备'}{row.current && <span>（当前设备）</span>}</> }, { key: 'created', title: '登录时间', render: (row) => new Date(row.auth_time).toLocaleString('zh-CN') }, { key: 'action', title: '操作', render: (row) => <ConfirmDialog trigger={<Button variant="danger">{row.current ? '退出当前设备' : '撤销会话'}</Button>} title={row.current ? '退出当前设备？' : '撤销这次会话？'} description={row.current ? '你需要重新登录当前设备。' : '该设备需要重新登录，其他设备不受影响。'} confirmLabel="确认撤销" onConfirm={() => revoke(row.id, row.current)} /> }]} rows={query.data?.items ?? []} rowKey={(row) => row.id} loading={query.isPending} unavailable={query.isError} onRetry={() => { void query.refetch(); }} emptyTitle="没有活动设备会话" emptyDescription="活动会话将显示在此处。" /><CursorPagination hasPrevious={cursors.length > 0} hasNext={query.data?.next_cursor !== null && query.data?.next_cursor !== undefined} loading={query.isFetching} onPrevious={() => { setCursors((values) => values.slice(0, -1)); }} onNext={() => { const next = query.data?.next_cursor; if (next) setCursors((values) => [...values, next]); }} /></section></>;
}
