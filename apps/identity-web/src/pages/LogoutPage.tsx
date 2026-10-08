import { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate, useSearchParams } from 'react-router';
import { z } from 'zod';
import { api, ApiError, logoutCompletedSchema } from '../lib/api';
import { Button } from '../components/Button';
import { Status } from '../components/Status';
import { PageTitle } from './PageTitle';

export default function LogoutPage() {
  const client = useQueryClient();
  const navigate = useNavigate();
  const [search] = useSearchParams();
  const values = search.getAll('confirmation');
  const id = values.length === 1 && z.uuid().safeParse(values[0]).success ? values[0] : undefined;
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<ApiError | null>(null);
  async function decide(decision: 'logout' | 'cancel') {
    if (!id || busy) return; setBusy(true); setFailure(null);
    try { const result = await api.requestProtocol('/oauth/logout/confirm', logoutCompletedSchema, { confirmation_id: id, decision }); if (decision === 'logout') { api.resetCsrf(); client.removeQueries({ queryKey: ['identity'] }); } if (result.redirect_to) window.location.assign(result.redirect_to); else await navigate(decision === 'logout' ? '/login' : '/me', { replace: true }); } catch (error) { setFailure(error instanceof ApiError ? error : new ApiError(0, 'CLIENT_NETWORK_UNAVAILABLE', '退出请求暂未完成，请重试')); } finally { setBusy(false); }
  }
  return <><PageTitle title="确认退出" /><h1>确认退出账号？</h1><p className="muted">打开退出链接不会自动撤销会话。确认后，当前身份会话及其应用授权将失效。</p>{id ? <div className="actions"><Button variant="primary" loading={busy} loadingLabel="正在退出…" onClick={() => { void decide('logout'); }}>确认退出</Button><Button disabled={busy} onClick={() => { void decide('cancel'); }}>取消</Button></div> : <Status kind="error" title="退出请求无效" description="请从应用重新发起退出请求。" />}{failure && <Status kind={failure.status === 503 || failure.status === 0 ? 'unavailable' : 'error'} title="退出未完成" description={failure.message} requestId={failure.requestId} />}<p><Link to="/me">返回账号安全</Link></p></>;
}
