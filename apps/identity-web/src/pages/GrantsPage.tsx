import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router';
import { api, ApiError, grantPageSchema, noContentSchema } from '../lib/api';
import { Button } from '../components/Button';
import { ConfirmDialog } from '../components/ConfirmDialog';
import { CursorPagination } from '../components/CursorPagination';
import { Status } from '../components/Status';
import { PageTitle } from './PageTitle';

export default function GrantsPage() {
  const client = useQueryClient();
  const [cursors, setCursors] = useState<string[]>([]);
  const [failure, setFailure] = useState<ApiError | null>(null);
  const cursor = cursors.at(-1);
  const query = useQuery({ queryKey: ['identity', 'grants', cursor ?? ''], queryFn: ({ signal }) => api.request(`/me/grants?limit=20${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ''}`, grantPageSchema, { signal }) });
  async function revoke(id: string) { try { await api.request(`/me/grants/${id}`, noContentSchema, { method: 'DELETE' }); setFailure(null); await client.invalidateQueries({ queryKey: ['identity', 'grants'] }); } catch (error) { setFailure(error instanceof ApiError ? error : new ApiError(0, 'CLIENT_NETWORK_UNAVAILABLE', '请求暂未完成')); throw error; } }
  return <><PageTitle title="应用授权" /><p className="eyebrow">ACCOUNT / GRANTS</p><h1>已授权应用</h1><p><Link to="/me">返回账号安全</Link></p><p className="muted">撤销后，应用下一次认证检查会立即失效。</p>{failure && <Status kind="error" title="撤销未完成" description={failure.message} requestId={failure.requestId} />}<section className="panel">{query.isPending ? <Status kind="loading" title="正在加载授权…" /> : query.isError ? <Status kind="unavailable" title="暂时无法加载授权" onRetry={() => { void query.refetch(); }} /> : query.data.items.length ? <ul>{query.data.items.map((grant) => <li key={grant.id}><h2>{grant.client_name}</h2><p>已授权范围：{grant.scope.join('、')}</p><ConfirmDialog trigger={<Button variant="danger">撤销授权</Button>} title={`撤销${grant.client_name}的授权？`} description="此授权的访问令牌与刷新令牌将失效，需要重新同意。其他应用授权不受影响。" confirmLabel="确认撤销" onConfirm={() => revoke(grant.id)} /></li>)}</ul> : <p>还没有授权应用。</p>}<CursorPagination hasPrevious={cursors.length > 0} hasNext={Boolean(query.data?.next_cursor)} loading={query.isFetching} onPrevious={() => { setCursors((values) => values.slice(0, -1)); }} onNext={() => { const cursor = query.data?.next_cursor; if (cursor) setCursors((values) => [...values, cursor]); }} /></section></>;
}
