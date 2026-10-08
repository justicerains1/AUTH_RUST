import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate } from 'react-router';
import { api, ApiError, meSchema, noContentSchema } from '../lib/api';
import { meQueryKey } from '../lib/query';
import { AccountAction } from '../components/AccountAction';
import { AccountLayout } from '../components/AccountLayout';
import { Button } from '../components/Button';
import { Status } from '../components/Status';

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
  return <AccountLayout title="账号安全" description="查看你的保护方式，管理每一次连接。"><section className="panel"><div className="security-header"><div><h2>保护你的账号</h2><p className="muted">{user.display_name ?? user.email}</p></div><span className={`badge ${security.totp_enabled || security.passkey_count > 0 ? '' : 'badge--neutral'}`}>{security.totp_enabled || security.passkey_count > 0 ? '已配置第二因素' : '建议增加账号保护'}</span></div><div className="detail-row"><div><strong>密码</strong><p>使用独立密码，修改后所有设备需重新登录。</p></div><Link className="button" to="/me/password/change">修改密码</Link></div><div className="detail-row"><div><strong>通行密钥</strong><p>{security.passkey_count} 个通行密钥，使用设备的指纹、面容或屏幕锁登录。</p></div><Link className="button" to="/me/passkeys">管理Passkey</Link></div><div className="detail-row"><div><strong>身份验证器</strong><p>{security.totp_enabled ? 'TOTP 已启用，用于生成一次性验证码。' : 'TOTP 未启用，可为密码登录增加一道保护。'}</p></div><Link className="button" to="/me/mfa">管理双因素验证与恢复码</Link></div><div className="detail-row"><div><strong>恢复码</strong><p>剩余 {security.recovery_codes_remaining} 个，用于无法使用验证器时完成验证。</p></div><Link className="button" to="/me/mfa">管理恢复码</Link></div></section><section className="panel"><h2>账号信息</h2><dl className="account-details"><dt>邮箱地址</dt><dd>{user.email}</dd><dt>邮箱状态</dt><dd>{user.email_verified ? '已验证' : '未验证'}</dd>{user.display_name !== undefined && <><dt>显示名称</dt><dd>{user.display_name}</dd></>}<dt>账号状态</dt><dd>{user.status === 'active' ? '正常' : '已禁用'}</dd><dt>创建时间</dt><dd>{new Date(user.created_at).toLocaleString('zh-CN')}</dd></dl></section><section className="panel"><div className="security-header"><div><h2>活跃会话</h2><p className="muted">当前设备：{session.user_agent ?? '浏览器'}</p></div><Link className="button" to="/me/sessions">查看设备会话</Link></div><p>当前会话到期：{new Date(session.expires_at).toLocaleString('zh-CN')}</p><p>发现不熟悉的设备时，可以撤销对应会话。</p></section><section className="panel"><div className="security-header"><div><h2>授权应用</h2><p className="muted">查看应用获准访问的账号信息，只保留仍在使用的连接。</p></div><Link className="button" to="/me/grants">查看已授权应用</Link></div></section><section className="panel"><h2>退出账号</h2><p>退出会立即撤销相应身份会话及其应用授权。</p><div className="actions"><AccountAction trigger={<Button>退出当前设备</Button>} title="退出当前设备？" description="当前设备需要重新登录，其他设备的会话继续保留。" confirmLabel="确认退出" onConfirm={() => logout(false)} /><AccountAction trigger={<Button variant="danger">退出所有设备</Button>} title="退出所有设备？" description="此账号的全部设备会话和派生应用授权将失效。" confirmLabel="退出所有设备" onConfirm={() => logout(true)} /></div></section></AccountLayout>;
}
