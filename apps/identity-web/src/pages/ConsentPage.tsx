import { useEffect, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Link, useParams } from 'react-router';
import { z } from 'zod';
import { api, ApiError, authorizationTransactionSchema, authorizationDecisionSchema } from '../lib/api';
import { asApiError, useRetryAfter } from '../lib/auth-feedback';
import { Button } from '../components/Button';
import { Status } from '../components/Status';
import { RequestFeedback } from '../components/RequestFeedback';
import { PageTitle } from './PageTitle';

const descriptions = { openid: '确认你的身份标识', profile: '读取账号显示名称', email: '读取邮箱地址与验证状态' } as const;
export default function ConsentPage() {
  const { id } = useParams();
  const validId = z.uuid().safeParse(id);
  const [now, setNow] = useState(Date.now);
  const [busy, setBusy] = useState(false);
  const active = useRef(false);
  const [failure, setFailure] = useState<ApiError | null>(null);
  const retry = useRetryAfter();
  const query = useQuery({ queryKey: ['identity', 'authorization', id], enabled: validId.success, queryFn: async ({ signal }) => { const value = await api.request(`/oauth/transactions/${id ?? ''}`, authorizationTransactionSchema, { signal }); if (value.id !== id) throw new ApiError(400, 'AUTH_ACTION_INVALID', '授权请求无效'); return value; } });
  useEffect(() => { const interval = window.setInterval(() => { setNow(Date.now()); }, 1000); return () => { window.clearInterval(interval); }; }, []);
  async function decide(decision: 'approve' | 'deny') {
    if (id === undefined || active.current || query.data === undefined || query.data.status !== 'consent_required' || now >= Date.parse(query.data.expires_at) || retry.remaining > 0) return;
    active.current = true; setBusy(true); setFailure(null);
    try { const response = await api.request(`/oauth/transactions/${id}/decision`, authorizationDecisionSchema, { method: 'POST', body: { decision } }); window.location.assign(response.redirect_to); }
    catch (error) { const detail = asApiError(error); retry.start(detail); setFailure(detail); }
    finally { active.current = false; setBusy(false); }
  }
  if (!validId.success) return <><PageTitle title="应用授权" /><Status kind="error" title="授权事务无效" description="请从应用重新发起授权。" /></>;
  if (query.isPending) return <Status kind="loading" title="正在加载授权请求…" />;
  if (query.isError) return <Status kind="unavailable" title="暂时无法读取授权请求" description={query.error instanceof ApiError ? query.error.message : '请从应用重新开始。'} onRetry={() => { void query.refetch(); }} />;
  const expired = Date.parse(query.data.expires_at) <= now;
  return <><PageTitle title="应用授权" /><div className="consent-layout"><div><p className="eyebrow">YOUR CHOICE, YOUR CONNECTION</p><h1>应用授权</h1><p className="lead muted">查看应用希望访问的信息，再决定是否继续。</p></div><section className="panel consent-panel"><h2>{query.data.client.name}</h2><p>此应用请求访问以下账号信息：</p><ul className="scope-list">{query.data.requested_scopes.map((scope) => <li key={scope}><span aria-hidden="true">✓</span>{descriptions[scope]}{query.data.previously_approved_scopes.includes(scope) && <span className="muted">（此前已同意）</span>}</li>)}</ul><p className="muted">授权请求到期：{new Date(query.data.expires_at).toLocaleString('zh-CN')}</p>{expired ? <Status kind="error" title="授权请求已过期" description="请回到应用重新发起连接。此次未完成授权。" /> : query.data.status === 'login_required' ? <Link className="button button--primary" to={`/login?transaction=${query.data.id}`}>登录后继续</Link> : <div className="actions"><Button variant="primary" loading={busy} disabled={retry.remaining > 0 || query.isFetching} loadingLabel="正在提交…" onClick={() => { void decide('approve'); }}>同意授权</Button><Button disabled={busy || retry.remaining > 0 || query.isFetching} onClick={() => { void decide('deny'); }}>拒绝授权</Button></div>}<RequestFeedback error={failure} title="授权未完成" remaining={retry.remaining} /></section></div></>;
}
