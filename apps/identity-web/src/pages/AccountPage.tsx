import { useQuery } from '@tanstack/react-query';
import { api, meSchema } from '../lib/api';
import { meQueryKey } from '../lib/query';
import { Empty } from '../components/Empty';
import { PageTitle } from './PageTitle';

export default function AccountPage() {
  const { data } = useQuery({ queryKey: meQueryKey, queryFn: ({ signal }) => api.request('/me', meSchema, { signal }) });
  return <><PageTitle title="账号安全" /><p className="eyebrow">ACCOUNT / SECURITY</p><h1>账号安全</h1>{data && <p className="muted">{data.user.display_name ?? data.user.email}</p>}<section className="panel"><Empty title="安全中心基础路由已就绪" description="认证器、设备会话和应用授权的管理将在对应任务完成后提供。" /></section></>;
}
