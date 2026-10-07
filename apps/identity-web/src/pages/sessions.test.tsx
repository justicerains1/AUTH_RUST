import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, cleanup } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { api, ApiError } from '../lib/api';
import LoginPage from './LoginPage';
import SessionsPage from './SessionsPage';

const uuid = '65d69320-97e8-4de0-a062-4c0f948a1b80';
function page(element: React.ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  render(<QueryClientProvider client={client}><MemoryRouter>{element}</MemoryRouter></QueryClientProvider>);
}
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

describe('T06 登录与会话控件边界', () => {
  it('密码请求失败后清理密码并保留邮箱及安全错误', async () => {
    const user = userEvent.setup();
    const request = vi.spyOn(api, 'request').mockRejectedValue(new ApiError(401, 'AUTH_INVALID_CREDENTIALS', '凭证无效，请重试'));
    page(<LoginPage />);
    await user.type(screen.getByLabelText('邮箱地址'), 'user@example.test');
    const password = screen.getByLabelText<HTMLInputElement>('密码');
    await user.type(password, 'uncommon candidate password');
    await user.click(screen.getByRole('button', { name: '登录' }));
    expect((await screen.findByRole('alert')).textContent).toContain('凭证无效');
    expect(password.value).toBe('');
    expect(screen.getByLabelText<HTMLInputElement>('邮箱地址').value).toBe('user@example.test');
    expect(request).toHaveBeenCalledTimes(1);
  });

  it('有限MFA分支清CSRF缓存且不呈现已登录账号', async () => {
    const user = userEvent.setup();
    vi.spyOn(api, 'request').mockResolvedValue({ status: 'mfa_required', challenge_id: uuid, purpose: 'login', methods: ['totp', 'recovery_code'], expires_at: '2026-10-08T12:05:00Z' });
    const reset = vi.spyOn(api, 'resetCsrf');
    page(<LoginPage />);
    await user.type(screen.getByLabelText('邮箱地址'), 'user@example.test');
    await user.type(screen.getByLabelText('密码'), 'uncommon candidate password');
    await user.click(screen.getByRole('button', { name: '登录' }));
    expect((await screen.findByRole('status')).textContent).toContain('此时尚未登录');
    expect(reset).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole('heading', { name: '账号安全' })).toBeNull();
    expect(screen.getByRole('button', { name: '重新开始登录' })).toBeTruthy();
  });

  it('限流失败显示服务器等待时间且不自动重放登录', async () => {
    const user = userEvent.setup();
    const request = vi.spyOn(api, 'request').mockRejectedValue(new ApiError(429, 'RATE_LIMITED', '请求较频繁', { retryAfter: 60 }));
    page(<LoginPage />);
    await user.type(screen.getByLabelText('邮箱地址'), 'user@example.test');
    await user.type(screen.getByLabelText('密码'), 'uncommon candidate password');
    await user.click(screen.getByRole('button', { name: '登录' }));
    expect((await screen.findByRole('status')).textContent).toContain('60秒后重试');
    expect(request).toHaveBeenCalledTimes(1);
  });

  it('会话撤销取消不调用API，确认后只撤销选中ID', async () => {
    const user = userEvent.setup();
    const request = vi.spyOn(api, 'request').mockImplementation((path) => {
      if (path.startsWith('/me/sessions?')) return Promise.resolve({ items: [{ id: uuid, amr: ['pwd'], auth_time: '2026-10-08T12:00:00Z', strong_at: null, expires_at: '2026-10-09T00:00:00Z', created_at: '2026-10-08T12:00:00Z', current: false, user_agent: 'Firefox' }], next_cursor: null });
      return Promise.resolve(undefined);
    });
    page(<SessionsPage />);
    await screen.findByRole('table');
    await user.click(screen.getByRole('button', { name: '撤销会话' }));
    await user.click(screen.getByRole('button', { name: '取消' }));
    expect(request.mock.calls.some(([path, , options]) => path === `/me/sessions/${uuid}` && options?.method === 'DELETE')).toBe(false);
    await user.click(screen.getByRole('button', { name: '撤销会话' }));
    await user.click(screen.getByRole('button', { name: '确认撤销' }));
    expect(request.mock.calls.some(([path, , options]) => path === `/me/sessions/${uuid}` && options?.method === 'DELETE')).toBe(true);
  });
});
