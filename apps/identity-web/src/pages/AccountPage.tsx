import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate } from 'react-router';
import { api, ApiError, meSchema, noContentSchema } from '../lib/api';
import { meQueryKey } from '../lib/query';
import { ConfirmDialog } from '../components/ConfirmDialog';
import { Button } from '../components/Button';
import { Status } from '../components/Status';
import { PageTitle } from './PageTitle';

export default function AccountPage() {
  const navigate = useNavigate();
  const client = useQueryClient();
  const query = useQuery({ queryKey: meQueryKey, queryFn: ({ signal }) => api.request('/me', meSchema, { signal }) });
  async function logout(all: boolean) {
    await api.request(all ? '/auth/logout-all' : '/auth/logout', noContentSchema, { method: 'POST' });
    api.resetCsrf(); client.removeQueries({ queryKey: ['identity'] }); await navigate('/login', { replace: true });
  }
  if (query.isPending) return <Status kind="loading" title="正在加载账号…" />;
  if (query.isError) return <Status kind="unavailable" title="暂时无法加载账号" description={query.error instanceof ApiError ? query.error.message : '请稍后重试'} onRetry={() => { void query.refetch(); }} />;
  const { user, security, session } = query.data;
  return <><PageTitle title="账号安全" /><p className="eyebrow">ACCOUNT / SECURITY</p><h1>账号安全</h1><p className="lead">{user.display_name ?? user.email}</p><div className="component-grid"><section className="panel"><h2>账号信息</h2><dl><dt>邮箱地址</dt><dd>{user.email}</dd><dt>邮箱状态</dt><dd>{user.email_verified ? '已验证' : '未验证'}</dd><dt>账号状态</dt><dd>{user.status === 'active' ? '正常' : '已禁用'}</dd></dl><p><Link to="/me/sessions">查看设备会话</Link></p><p><Link to="/me/password/change">修改密码</Link></p><p><Link to="/me/mfa">管理双因素验证与恢复码</Link></p></section><section className="panel"><h2>登录与安全状态</h2><dl><dt>TOTP</dt><dd>{security.totp_enabled ? '已启用' : '未启用'}</dd><dt>Passkey 数量</dt><dd>{security.passkey_count}</dd><dt>剩余恢复码</dt><dd>{security.recovery_codes_remaining}</dd><dt>当前会话到期</dt><dd>{new Date(session.expires_at).toLocaleString('zh-CN')}</dd></dl></section><section className="panel"><h2>退出账号</h2><p>退出会立即撤销相应身份会话及其应用授权。</p><div className="actions"><ConfirmDialog trigger={<Button>退出当前设备</Button>} title="退出当前设备？" description="当前设备需要重新登录，其他设备的会话继续保留。" confirmLabel="确认退出" onConfirm={() => logout(false)} /><ConfirmDialog trigger={<Button variant="danger">退出所有设备</Button>} title="退出所有设备？" description="此账号的全部设备会话和派生应用授权将失效。" confirmLabel="退出所有设备" onConfirm={() => logout(true)} /></div></section></div></>;
}
