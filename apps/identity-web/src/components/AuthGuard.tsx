import { useQuery } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { Link, Navigate, Outlet, useLocation } from 'react-router';
import { api, ApiError, meSchema } from '../lib/api';
import { meQueryKey } from '../lib/query';
import { Status } from './Status';
import { Empty } from './Empty';
import { Button } from './Button';
import { ReauthenticationDialog } from './ReauthenticationDialog';
import { factorMethods } from '../lib/reauthentication';

export function AuthGuard({ admin = false }: { admin?: boolean }) {
  const [now, setNow] = useState(() => Date.now());
  const [reauthenticationOpen, setReauthenticationOpen] = useState(false);
  const location = useLocation();
  const query = useQuery({ queryKey: meQueryKey, queryFn: ({ signal }) => api.request('/me', meSchema, { signal }) });
  useEffect(() => {
    const interval = window.setInterval(() => { setNow(Date.now()); }, 1000);
    return () => { window.clearInterval(interval); };
  }, []);
  if (query.isPending) return <Status kind="loading" title="正在检查账号状态…" />;
  if (query.isError) {
    if (query.error instanceof ApiError && query.error.status === 401) return <Navigate to="/login" replace />;
    if (query.error instanceof ApiError && query.error.status === 403) return <Empty title="无法访问此页面" description="当前账号没有此页面所需的权限。" />;
    return <Status kind="unavailable" title="暂时无法检查账号状态" description="请重新加载后再继续。" onRetry={() => { void query.refetch(); }} retrying={query.isFetching} />;
  }
  if (!query.data.user.email_verified || query.data.user.status !== 'active' || Date.parse(query.data.session.expires_at) <= now) return <Navigate to="/login" replace />;
  if (!admin && query.data.security.admin_binding_only && !['/me/mfa', '/me/passkeys'].includes(location.pathname)) return <Navigate to="/me/mfa" replace />;
  if (admin) {
    if (!query.data.security.is_admin) return <Empty title="无法访问管理后台" description="管理后台仅对合格管理员开放。" />;
    if (query.data.security.admin_binding_only) return <Empty title="请先绑定认证因素" description="首个管理员需先绑定身份验证器或通行密钥，才能使用管理后台。" action={<Link className="button button--primary" to="/me/mfa">绑定账号保护</Link>} />;
    const strongAt = query.data.session.strong_at === null ? NaN : Date.parse(query.data.session.strong_at);
    if (!Number.isFinite(strongAt) || strongAt > now || now - strongAt >= 300_000) return <><Empty title="请先完成近期强认证" description="管理后台需要最近五分钟内的强认证。" action={<Button onClick={() => { setReauthenticationOpen(true); }}>确认当前身份</Button>} /><ReauthenticationDialog open={reauthenticationOpen} onOpenChange={setReauthenticationOpen} requiredStrength="strong" methods={factorMethods(query.data)} onConfirmed={async () => { await query.refetch(); }} /></>;
  }
  return <Outlet />;
}
