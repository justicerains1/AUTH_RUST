import { StrictMode } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { api, ApiError } from '../lib/api';
import LoginPage from './LoginPage';
import RegisterPage from './RegisterPage';
import EmailVerificationPage from './EmailVerificationPage';
import PasswordResetPage from './PasswordResetPage';

function page(element: React.ReactNode, path = '/') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  return render(<StrictMode><QueryClientProvider client={client}><MemoryRouter initialEntries={[path]}>{element}</MemoryRouter></QueryClientProvider></StrictMode>);
}
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.useRealTimers(); window.history.replaceState(null, '', '/'); });

describe('T17 公开认证状态边界（UI 单元响应）', () => {
  it('StrictMode打开邮件只清fragment，明确确认后只发送一次并完成验证', async () => {
    window.history.replaceState(null, '', `/email-verification#token=${'A'.repeat(43)}`);
    const request = vi.spyOn(api, 'request').mockResolvedValue({ status: 'verified' });
    page(<EmailVerificationPage />);
    expect(window.location.hash).toBe('');
    expect(request).not.toHaveBeenCalled();
    await userEvent.setup().click(screen.getByRole('button', { name: '确认验证邮箱' }));
    expect(await screen.findByText('邮箱验证完成')).toBeTruthy();
    expect(request).toHaveBeenCalledTimes(1);
  });
  it('错误或重复fragment不被提交，刷新后的页面允许重新申请邮件', () => {
    window.history.replaceState(null, '', `/email-verification#token=${'A'.repeat(43)}&token=${'B'.repeat(43)}`);
    const request = vi.spyOn(api, 'request');
    page(<EmailVerificationPage />);
    expect(screen.queryByRole('button', { name: '确认验证邮箱' })).toBeNull();
    expect(screen.getByText('验证链接无效或已过期')).toBeTruthy();
    expect(request).not.toHaveBeenCalled();
  });
  it('服务器Retry-After期间禁重复提交，倒计时结束不自动重试且保留邮箱', async () => {
    const request = vi.spyOn(api, 'request').mockRejectedValue(new ApiError(429, 'RATE_LIMITED', '请求较频繁', { retryAfter: 1 }));
    const user = userEvent.setup();
    page(<RegisterPage />);
    const email = screen.getByLabelText<HTMLInputElement>('邮箱地址');
    await user.type(email, 'fixture@example.test');
    await user.type(screen.getByLabelText('密码', { exact: true }), 'fixture long passphrase');
    await user.click(screen.getByRole('button', { name: '创建账号' }));
    expect(screen.getByText(/1 秒后可以重试/u)).toBeTruthy();
    expect(screen.getByRole<HTMLButtonElement>('button', { name: '创建账号' }).disabled).toBe(true);
    expect(screen.getByLabelText<HTMLInputElement>('密码', { exact: true }).value).toBe('');
    await waitFor(() => { expect(screen.getByRole<HTMLButtonElement>('button', { name: '创建账号' }).disabled).toBe(false); }, { timeout: 1600 });
    expect(request).toHaveBeenCalledTimes(1);
    expect(email.value).toBe('fixture@example.test');
  });
  it('只有服务端确认的OAuth事务会保留到注册和找回链接', async () => {
    const id = '65d69320-97e8-4de0-a062-4c0f948a1b80';
    const request = vi.spyOn(api, 'request').mockImplementation((path) => path === '/me' ? Promise.reject(new ApiError(401, 'AUTH_SESSION_REQUIRED', '请登录')) : Promise.resolve({ id, client: { client_id: 'test-app', name: '应用测试' }, requested_scopes: ['openid'], previously_approved_scopes: [], expires_at: new Date(Date.now() + 60_000).toISOString(), status: 'login_required' }));
    page(<LoginPage />, `/login?transaction=${id}`);
    await waitFor(() => { expect(screen.getByRole('link', { name: '创建账号' }).getAttribute('href')).toBe(`/register?transaction=${id}`); });
    expect(screen.getByRole('link', { name: '忘记密码？' }).getAttribute('href')).toBe(`/password-reset?transaction=${id}`);
    expect(request.mock.calls.some(([path]) => path === `/oauth/transactions/${id}`)).toBe(true);
  });
  it('找回邮件成功使用服务端统一文案，不推断邮箱是否存在', async () => {
    const message = '如果该邮箱可接收此操作的邮件，我们将发送后续说明。';
    vi.spyOn(api, 'request').mockResolvedValue({ status: 'accepted', message });
    page(<PasswordResetPage />);
    const user = userEvent.setup();
    await user.type(screen.getByLabelText('邮箱地址'), 'fixture@example.test');
    await user.click(screen.getByRole('button', { name: '发送重置邮件' }));
    expect(await screen.findByText(message)).toBeTruthy();
  });
});
