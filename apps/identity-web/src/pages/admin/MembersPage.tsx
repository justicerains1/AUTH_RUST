import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { adminApi } from '../../lib/admin-api';
import { Input } from '../../components/Input';
import { Status } from '../../components/Status';
import { CursorPagination } from '../../components/CursorPagination';
import { Empty } from '../../components/Empty';
import { AdminAction, AdminFailure, AdminPanel, AdminTable } from './AdminShared';
import { PageTitle } from '../PageTitle';

export default function MembersPage() {
  const [target, setTarget] = useState(''); const [cursor, setCursor] = useState<string | undefined>(); const [history, setHistory] = useState<(string | undefined)[]>([]);
  const query = useQuery({ queryKey: ['admin', 'members', cursor], queryFn: ({ signal }) => adminApi.members(cursor, signal), retry: false });
  return <><PageTitle title="管理员成员" /><h1>管理员成员</h1><p className="muted">仅可授予已验证并配置强因素的已有账号。</p><AdminPanel><Input label="已有用户ID" value={target} onChange={(event) => { setTarget(event.target.value); }} /><AdminAction label="授予管理员" title="授予此用户管理员权限？" description="管理员能够管理用户状态、应用与安全审计。请核对用户ID。" action={() => adminApi.grantMember(target)} onComplete={async () => { setTarget(''); await query.refetch(); }} /></AdminPanel><AdminPanel>
    {query.isPending ? <Status kind="loading" title="正在读取成员…" /> : query.isError ? <AdminFailure error={query.error} retry={() => { void query.refetch(); }} /> : query.data.items.length === 0 ? <Empty title="没有管理员成员" description="请检查后台成员配置。" /> : <AdminTable caption="管理员列表" columns={[{ key: 'email', header: '邮箱' }, { key: 'status', header: '状态' }, { key: 'action', header: '操作' }]} rows={query.data.items.map((member) => ({ id: member.user_id, cells: { email: member.email, status: member.enabled ? '已启用' : '已停用', action: <AdminAction label="移除管理员" title="移除此管理员？" description="用户仍保留普通账号，后台权限将失效；最后可用管理员不能移除。" danger action={() => adminApi.removeMember(member.user_id)} onComplete={async () => { await query.refetch(); }} /> } }))} />}
    <CursorPagination hasPrevious={history.length > 0} hasNext={query.data?.next_cursor != null} loading={query.isFetching} onPrevious={() => { setCursor(history.at(-1)); setHistory(history.slice(0, -1)); }} onNext={() => { if (query.data?.next_cursor) { setHistory([...history, cursor]); setCursor(query.data.next_cursor); } }} />
  </AdminPanel></>;
}
