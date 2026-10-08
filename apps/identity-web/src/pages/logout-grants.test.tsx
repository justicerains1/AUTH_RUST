import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, cleanup } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router';
import { afterEach, expect, it, vi } from 'vitest';
import { api, ApiError } from '../lib/api';
import LogoutPage from './LogoutPage';
import GrantsPage from './GrantsPage';
const id = '65d69320-97e8-4de0-a062-4c0f948a1b80';
function page(element: React.ReactNode, route = '/') { const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } }); render(<QueryClientProvider client={client}><MemoryRouter initialEntries={[route]}>{element}</MemoryRouter></QueryClientProvider>); }
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

it('退出确认页面打开不提交，用户确认时才发送受限协议路径', async () => {
  const user = userEvent.setup();
  const request = vi.spyOn(api, 'requestProtocol').mockRejectedValue(new ApiError(503, 'temporarily_unavailable', '退出暂不可用'));
  page(<LogoutPage />, `/logout?confirmation=${id}`);
  expect(request).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: '确认退出' }));
  expect((await screen.findByRole('status')).textContent).toContain('退出暂不可用');
  expect(request).toHaveBeenCalledWith('/oauth/logout/confirm', expect.anything(), { confirmation_id: id, decision: 'logout' });
  expect(request).toHaveBeenCalledTimes(1);
});

it('取消退出发送明确cancel，不被视作已退出成功', async () => {
  const user = userEvent.setup();
  const request = vi.spyOn(api, 'requestProtocol').mockRejectedValue(new ApiError(400, 'invalid_request', '确认已失效'));
  page(<LogoutPage />, `/logout?confirmation=${id}`);
  await user.click(screen.getByRole('button', { name: '取消' }));
  expect((await screen.findByRole('alert')).textContent).toContain('确认已失效');
  expect(request.mock.calls[0]?.[2].decision).toBe('cancel');
});

it('授权撤销Dialog取消不调用撤销，确认只提交该授权ID', async () => {
  const user = userEvent.setup();
  const request = vi.spyOn(api, 'request').mockImplementation((path) => path.startsWith('/me/grants?') ? Promise.resolve({ items: [{ id, client_id: 'app-a', client_name: '应用 A', scope: ['openid'], created_at: '2026-10-08T12:00:00Z', expires_at: '2026-10-09T00:00:00Z' }], next_cursor: null }) : Promise.resolve(undefined));
  page(<GrantsPage />);
  await screen.findByText('应用 A');
  await user.click(screen.getByRole('button', { name: '撤销授权' }));
  await user.click(screen.getByRole('button', { name: '取消' }));
  expect(request.mock.calls.some(([path]) => path === `/me/grants/${id}`)).toBe(false);
  await user.click(screen.getByRole('button', { name: '撤销授权' }));
  await user.click(screen.getByRole('button', { name: '确认撤销' }));
  expect(request.mock.calls.some(([path, , options]) => path === `/me/grants/${id}` && options?.method === 'DELETE')).toBe(true);
});
