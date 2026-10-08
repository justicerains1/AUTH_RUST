import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, cleanup } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes } from 'react-router';
import { afterEach, expect, it, vi } from 'vitest';
import { api, ApiError } from '../lib/api';
import ConsentPage from './ConsentPage';

const id = '65d69320-97e8-4de0-a062-4c0f948a1b80';
afterEach(() => { cleanup(); vi.restoreAllMocks(); });
function page() { const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } }); render(<QueryClientProvider client={client}><MemoryRouter initialEntries={[`/oauth/consent/${id}`]}><Routes><Route path="/oauth/consent/:id" element={<ConsentPage />} /></Routes></MemoryRouter></QueryClientProvider>); }
const transaction = { id, client: { client_id: 'demo-a', name: '<script>Application</script>' }, requested_scopes: ['openid', 'email'], previously_approved_scopes: [], expires_at: new Date(Date.now() + 60_000).toISOString(), status: 'consent_required' };

it('同意页面展示服务端客户端/范围且不自动提交授权决定', async () => {
  const request = vi.spyOn(api, 'request').mockResolvedValue(transaction);
  page();
  expect((await screen.findByRole('heading', { level: 2 })).textContent).toBe('<script>Application</script>');
  expect(screen.getByText('读取邮箱地址与验证状态')).toBeTruthy();
  expect(document.querySelector('script')).toBeNull();
  expect(request).toHaveBeenCalledTimes(1);
  expect(screen.getByRole('button', { name: '同意授权' })).toBeTruthy();
});

it('拒绝明确提交deny，失败不自动重放或伪跳转', async () => {
  const user = userEvent.setup();
  const request = vi.spyOn(api, 'request').mockResolvedValueOnce(transaction).mockRejectedValueOnce(new ApiError(503, 'DEPENDENCY_UNAVAILABLE', '暂不可用'));
  page();
  await screen.findByRole('button', { name: '拒绝授权' });
  await user.click(screen.getByRole('button', { name: '拒绝授权' }));
  expect((await screen.findByRole('status')).textContent).toContain('暂不可用');
  expect(request.mock.calls[1]?.[2]?.body).toEqual({ decision: 'deny' });
  expect(request).toHaveBeenCalledTimes(2);
});

it('服务端要求登录时保留UUID续接链接，不显示批准按钮', async () => {
  vi.spyOn(api, 'request').mockResolvedValue({ ...transaction, status: 'login_required' });
  page();
  const link = await screen.findByRole('link', { name: '登录后继续' });
  expect(link.getAttribute('href')).toBe(`/login?transaction=${id}`);
  expect(screen.queryByRole('button', { name: '同意授权' })).toBeNull();
});
