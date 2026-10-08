import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { adminApi } from '../../lib/admin-api';
import { Input } from '../../components/Input';
import { Button } from '../../components/Button';
import { Status } from '../../components/Status';
import { CursorPagination } from '../../components/CursorPagination';
import { Empty } from '../../components/Empty';
import { AdminFailure, AdminPanel, AdminTable } from './AdminShared';
import { PageTitle } from '../PageTitle';

function initialWindow() { const end = new Date(); return { from: new Date(end.getTime() - 7 * 86_400_000).toISOString(), to: end.toISOString() }; }
export default function AuditPage() {
  const [window, setWindow] = useState(initialWindow); const [from, setFrom] = useState(window.from); const [to, setTo] = useState(window.to); const [error, setError] = useState<string | null>(null); const [cursor, setCursor] = useState<string | undefined>(); const [history, setHistory] = useState<(string | undefined)[]>([]);
  const query = useQuery({ queryKey: ['admin', 'audit', window, cursor], queryFn: ({ signal }) => adminApi.audit({ ...window, cursor }, signal), retry: false });
  return <><PageTitle title="安全审计" /><h1>安全审计</h1><p className="muted">按固定时间窗口逐页读取，不下载全部历史。最多查询31天。</p><AdminPanel><form onSubmit={(event) => { event.preventDefault(); const start = Date.parse(from); const end = Date.parse(to); if (!Number.isFinite(start) || !Number.isFinite(end) || start >= end || end - start > 31 * 86_400_000) { setError('请填写有效的RFC3339时间窗口，跨度不超过31天。'); return; } setError(null); setWindow({ from: new Date(start).toISOString(), to: new Date(end).toISOString() }); setCursor(undefined); setHistory([]); }}><Input label="开始时间（RFC3339）" value={from} onChange={(event) => { setFrom(event.target.value); }} /><Input label="结束时间（RFC3339）" value={to} onChange={(event) => { setTo(event.target.value); }} /><Button type="submit">查询审计</Button></form>{error && <Status kind="error" title="查询时间无效" description={error} />}
    {query.isPending ? <Status kind="loading" title="正在读取审计…" /> : query.isError ? <AdminFailure error={query.error} retry={() => { void query.refetch(); }} /> : query.data.items.length === 0 ? <Empty title="该时间窗口暂无审计记录" description="可以调整时间窗口后重新查询。" /> : <AdminTable caption="审计事件" columns={[{ key: 'time', header: '发生时间' }, { key: 'event', header: '事件' }, { key: 'result', header: '结果' }, { key: 'source', header: '来源' }]} rows={query.data.items.map((event) => ({ id: event.id, cells: { time: new Date(event.occurred_at).toLocaleString('zh-CN'), event: event.event, result: event.result, source: event.source } }))} />}
    <CursorPagination hasPrevious={history.length > 0} hasNext={query.data?.next_cursor != null} loading={query.isFetching} onPrevious={() => { setCursor(history.at(-1)); setHistory(history.slice(0, -1)); }} onNext={() => { if (query.data?.next_cursor) { setHistory([...history, cursor]); setCursor(query.data.next_cursor); } }} />
  </AdminPanel></>;
}
