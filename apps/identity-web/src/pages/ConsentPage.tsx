import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Link, useParams } from 'react-router';
import { z } from 'zod';
import { api, ApiError, authorizationTransactionSchema, authorizationDecisionSchema } from '../lib/api';
import { Button } from '../components/Button';
import { Status } from '../components/Status';
import { PageTitle } from './PageTitle';

const descriptions = { openid: '确认你的身份标识', profile: '读取账号显示名称', email: '读取邮箱地址与验证状态' } as const;
export default function ConsentPage() {
  const { id } = useParams();
  const validId = z.uuid().safeParse(id);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<ApiError | null>(null);
  const query = useQuery({ queryKey: ['identity', 'authorization', id], enabled: validId.success, queryFn: ({ signal }) => api.request(`/oauth/transactions/${id ?? ''}`, authorizationTransactionSchema, { signal }) });
  async function decide(decision: 'approve' | 'deny') {
    if (!id || busy) return; setBusy(true); setFailure(null);
    try { const response = await api.request(`/oauth/transactions/${id}/decision`, authorizationDecisionSchema, { method: 'POST', body: { decision } }); window.location.assign(response.redirect_to); } catch (error) { setFailure(error instanceof ApiError ? error : new ApiError(0, 'CLIENT_NETWORK_UNAVAILABLE', '授权决定暂未完成，请重试')); } finally { setBusy(false); }
  }
  if (!validId.success) return <Status kind="error" title="授权事务无效" description="请从应用重新发起授权。" />;
  if (query.isPending) return <Status kind="loading" title="正在加载授权请求…" />;
  if (query.isError) return <Status kind="unavailable" title="暂时无法读取授权请求" description={query.error instanceof ApiError ? query.error.message : '请从应用重新开始。'} onRetry={() => { void query.refetch(); }} />;
  return <><PageTitle title="应用授权" /><p className="eyebrow">ACCOUNT / APPLICATION CONSENT</p><h1>应用授权</h1><section className="panel form-panel"><h2>{query.data.client.name}</h2><p>此应用请求访问以下账号信息：</p><ul>{query.data.requested_scopes.map((scope) => <li key={scope}>{descriptions[scope]}</li>)}</ul><p className="muted">授权请求到期：{new Date(query.data.expires_at).toLocaleString('zh-CN')}</p>{query.data.status === 'login_required' ? <Link className="button button--primary" to={`/login?transaction=${query.data.id}`}>登录后继续</Link> : <div className="actions"><Button variant="primary" loading={busy} loadingLabel="正在提交…" onClick={() => { void decide('approve'); }}>同意授权</Button><Button disabled={busy} onClick={() => { void decide('deny'); }}>拒绝授权</Button></div>}{failure && <Status kind={failure.status === 429 ? 'limited' : failure.status === 503 || failure.status === 0 ? 'unavailable' : 'error'} title="授权未完成" description={failure.message} requestId={failure.requestId} />}</section></>;
}
