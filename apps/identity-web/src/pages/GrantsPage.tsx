import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, grantPageSchema, noContentSchema } from '../lib/api';
import { Button } from '../components/Button';
import { AccountAction } from '../components/AccountAction';
import { AccountLayout } from '../components/AccountLayout';
import { CursorPagination } from '../components/CursorPagination';
import { Empty } from '../components/Empty';
import { Status } from '../components/Status';

const labels = { openid: '身份标识', profile: '显示名称', email: '邮箱地址与验证状态' } as const;
export default function GrantsPage() {
  const client = useQueryClient();
  const [cursors, setCursors] = useState<string[]>([]);
  const cursor = cursors.at(-1);
  const query = useQuery({ queryKey: ['identity', 'grants', cursor ?? ''], queryFn: ({ signal }) => api.request(`/me/grants?limit=20${cursor === undefined ? '' : `&cursor=${encodeURIComponent(cursor)}`}`, grantPageSchema, { signal }) });
  async function revoke(id: string) { await api.request(`/me/grants/${id}`, noContentSchema, { method: 'DELETE' }); await client.invalidateQueries({ queryKey: ['identity', 'grants'] }); }
  return <AccountLayout title="已授权应用" description="只保留仍在使用的连接。撤销后，应用下一次认证检查会立即失效。"><section className="panel"><h2>你的应用连接</h2>{query.isPending ? <Status kind="loading" title="正在加载授权…" /> : query.isError ? <Status kind="unavailable" title="暂时无法加载授权" onRetry={() => { void query.refetch(); }} /> : query.data.items.length > 0 ? <ul className="grant-list">{query.data.items.map((grant) => <li className="detail-row" key={grant.id}><div><h3>{grant.client_name}</h3><p>已授权范围：{grant.scope.map((scope) => labels[scope]).join('、')}</p><p>授权时间：{new Date(grant.created_at).toLocaleString('zh-CN')}</p></div><AccountAction trigger={<Button variant="danger">撤销授权</Button>} title={`撤销${grant.client_name}的授权？`} description="此授权的访问令牌与刷新令牌将失效，继续连接需要重新同意。其他应用授权不受影响。" confirmLabel="确认撤销" onConfirm={() => revoke(grant.id)} /></li>)}</ul> : <Empty title="还没有授权应用" description="连接应用并明确同意后，授权会显示在这里。" />}<CursorPagination hasPrevious={cursors.length > 0} hasNext={Boolean(query.data?.next_cursor)} loading={query.isFetching} onPrevious={() => { setCursors((values) => values.slice(0, -1)); }} onNext={() => { const next = query.data?.next_cursor; if (next) setCursors((values) => [...values, next]); }} /></section></AccountLayout>;
}
