import { lazy, Suspense, useState } from 'react';
import { Button } from '../../components/Button';
import { Status } from '../../components/Status';

const Users = lazy(() => import('./UsersPage')); const Clients = lazy(() => import('./ClientsPage')); const Members = lazy(() => import('./MembersPage')); const Audit = lazy(() => import('./AuditPage'));
export default function AdminConsole() { const [tab, setTab] = useState<'users' | 'clients' | 'members' | 'audit'>('users'); return <><p className="eyebrow">PLATFORM / ADMINISTRATION</p><nav className="actions admin-navigation" aria-label="管理后台导航">{([['users', '用户'], ['clients', '应用客户端'], ['members', '管理员'], ['audit', '安全审计']] as const).map(([key, label]) => <Button key={key} aria-current={tab === key ? 'page' : undefined} onClick={() => { setTab(key); }}>{label}</Button>)}</nav><Suspense fallback={<Status kind="loading" title="正在加载后台页面…" />}>{tab === 'users' ? <Users /> : tab === 'clients' ? <Clients /> : tab === 'members' ? <Members /> : <Audit />}</Suspense></>; }
