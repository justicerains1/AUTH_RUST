import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, expect, it, vi } from 'vitest';
import ClientsPage from './ClientsPage';
import { AdminAction } from './AdminShared';
import { adminApi, adminClientSchema, auditEventSchema } from '../../lib/admin-api';
import { api } from '../../lib/api';

afterEach(() => { cleanup(); vi.restoreAllMocks(); });
const userId = '65d69320-97e8-4de0-a062-4c0f948a1b80';
const client = { id: userId, client_id: 'client-example', name: '<img src=x onerror=alert(1)>', enabled: true, allowed_scopes: ['openid'] as const, redirect_uris: ['https://app.example/callback'], post_logout_redirect_uris: [], created_at: '2026-10-08T12:00:00Z' };

it('admin client secrets appear only in the local result dialog and vanish on close', async () => {
  vi.spyOn(adminApi, 'clients').mockResolvedValue({ items: [], next_cursor: null });
  const secret = 'A'.repeat(43); const create = vi.spyOn(adminApi, 'createClient').mockResolvedValue({ client: { ...client, allowed_scopes: ['openid'] }, client_secret: secret, shown_once: true });
  vi.spyOn(api, 'request').mockResolvedValue({ status: 'reauthenticated', strong: true, reauthenticated_at: '2026-10-08T12:00:00Z', valid_until: '2026-10-08T12:05:00Z', amr: ['user'] });
  const cache = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(<QueryClientProvider client={cache}><ClientsPage /></QueryClientProvider>); const user = userEvent.setup();
  await user.type(screen.getByLabelText('应用名称'), '演示应用'); await user.type(screen.getByLabelText('登录回调地址（每行一个）'), 'https://app.example/callback');
  await user.click(screen.getByRole('button', { name: '创建客户端' })); expect(create).not.toHaveBeenCalled(); await user.type(screen.getByLabelText('当前密码'), 'synthetic test password'); await user.click(screen.getByRole('button', { name: '确认密码' }));
  await user.click(await screen.findByRole('button', { name: '创建客户端' })); expect(await screen.findByText(secret)).toBeTruthy();
  expect(create).toHaveBeenCalledOnce(); expect(JSON.stringify(cache.getQueryCache().getAll().map((query) => query.state.data))).not.toContain(secret); expect(cache.getMutationCache().getAll()).toHaveLength(0);
  await user.click(screen.getByRole('button', { name: '已保存，关闭秘密' })); expect(screen.queryByText(secret)).toBeNull(); expect(Object.keys(localStorage)).toEqual([]); expect(Object.keys(sessionStorage)).toEqual([]);
});

it('server-provided client names render as escaped text', async () => {
  vi.spyOn(adminApi, 'clients').mockResolvedValue({ items: [{ ...client, allowed_scopes: ['openid'] }], next_cursor: null });
  const cache = new QueryClient({ defaultOptions: { queries: { retry: false } } }); render(<QueryClientProvider client={cache}><ClientsPage /></QueryClientProvider>);
  expect(await screen.findByText(client.name)).toBeTruthy(); expect(document.querySelector('img')).toBeNull(); expect(adminClientSchema.parse({ ...client, allowed_scopes: ['openid'] }).name).toBe(client.name);
});

it('a high-risk admin action requires identity confirmation and then a separate explicit confirmation', async () => {
  const action = vi.fn().mockResolvedValue(undefined); const completed = vi.fn();
  vi.spyOn(api, 'request').mockResolvedValue({ status: 'reauthenticated', strong: true, reauthenticated_at: '2026-10-08T12:00:00Z', valid_until: '2026-10-08T12:05:00Z', amr: ['user'] });
  render(<AdminAction label="停用客户端" title="停用该应用？" description="该应用凭证检查将失效。" action={action} onComplete={completed} />); const user = userEvent.setup();
  await user.click(screen.getByRole('button', { name: '停用客户端' })); await user.click(screen.getByRole('button', { name: '确认身份后继续' }));
  expect(screen.getAllByRole('dialog')).toHaveLength(1); await user.type(screen.getByLabelText('当前密码'), 'synthetic test password'); await user.click(screen.getByRole('button', { name: '确认密码' }));
  expect(await screen.findByRole('button', { name: '确认停用客户端' })).toBeTruthy(); expect(action).not.toHaveBeenCalled(); expect(screen.getAllByRole('dialog')).toHaveLength(1);
  await user.click(screen.getByRole('button', { name: '确认停用客户端' })); expect(action).toHaveBeenCalledOnce(); expect(completed).toHaveBeenCalledOnce();
});

it('worker delivery audit targets remain readable after real mail delivery', () => {
  const event = auditEventSchema.parse({ id:userId,event:'email.delivery_succeeded',actor_user_id:null,target_type:'outbox',target_id:userId,result:'success',request_id:userId,source:'worker',occurred_at:'2026-10-08T12:00:00Z' });
  expect(event.target_type).toBe('outbox');
});
