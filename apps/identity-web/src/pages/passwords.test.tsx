import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, cleanup } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { api, ApiError } from '../lib/api';
import PasswordResetPage from './PasswordResetPage';
import PasswordChangePage from './PasswordChangePage';

function page(element: React.ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
  render(<QueryClientProvider client={client}><MemoryRouter>{element}</MemoryRouter></QueryClientProvider>);
}
afterEach(() => { cleanup(); vi.restoreAllMocks(); window.history.replaceState(null, '', '/'); });

describe('T07 密码操作页面边界', () => {
  it('打开fragment立即清除，不自动消费且用户提交后清理新密码', async () => {
    const token = 'A'.repeat(43);
    window.history.replaceState(null, '', `/password-reset#token=${token}`);
    const request = vi.spyOn(api, 'request').mockRejectedValue(new ApiError(503, 'DEPENDENCY_UNAVAILABLE', '服务暂不可用'));
    const user = userEvent.setup();
    page(<PasswordResetPage />);
    expect(window.location.hash).toBe('');
    expect(request).not.toHaveBeenCalled();
    const password = screen.getByLabelText<HTMLInputElement>('新密码');
    await user.type(password, 'uncommon fixture new password');
    await user.click(screen.getByRole('button', { name: '重置密码' }));
    expect((await screen.findByRole('status')).textContent).toContain('服务暂不可用');
    expect(password.value).toBe('');
    expect(request).toHaveBeenCalledTimes(1);
  });

  it('邮箱申请网络失败保留邮箱且不自动重试危险请求', async () => {
    const user = userEvent.setup();
    const request = vi.spyOn(api, 'request').mockRejectedValue(new ApiError(0, 'CLIENT_NETWORK_UNAVAILABLE', '网络暂不可用'));
    page(<PasswordResetPage />);
    const email = screen.getByLabelText<HTMLInputElement>('邮箱地址');
    await user.type(email, 'user@example.test');
    await user.click(screen.getByRole('button', { name: '发送重置邮件' }));
    expect((await screen.findByRole('status')).textContent).toContain('网络暂不可用');
    expect(email.value).toBe('user@example.test');
    expect(request).toHaveBeenCalledTimes(1);
  });

  it('已有MFA的密码确认不解锁修改按钮', async () => {
    const user = userEvent.setup();
    vi.spyOn(api, 'request').mockResolvedValue({ status: 'mfa_required', challenge_id: '65d69320-97e8-4de0-a062-4c0f948a1b80', purpose: 'reauthentication', methods: ['totp'], expires_at: '2026-10-08T12:05:00Z' });
    page(<PasswordChangePage />);
    const current = screen.getByLabelText<HTMLInputElement>('当前密码');
    await user.type(current, 'uncommon current password');
    await user.click(screen.getByRole('button', { name: '确认当前密码' }));
    expect((await screen.findByRole('status')).textContent).toContain('不能仅凭密码修改密码');
    expect(current.value).toBe('');
    expect(screen.getByRole<HTMLButtonElement>('button', { name: '修改密码' }).disabled).toBe(true);
    expect(screen.getByLabelText<HTMLInputElement>('新密码').disabled).toBe(true);
  });

  it('过期近期认证按403响应重新锁定新密码操作', async () => {
    const user = userEvent.setup();
    const request = vi.spyOn(api, 'request').mockResolvedValueOnce({ status: 'reauthenticated', reauthenticated_at: '2026-10-08T12:00:00Z', valid_until: '2026-10-08T12:05:00Z', strong: false, amr: ['pwd'] }).mockRejectedValueOnce(new ApiError(403, 'AUTH_REAUTH_REQUIRED', '请先完成近期重新认证', { next: { status: 'reauth_required', required_strength: 'password', methods: ['password'] } }));
    page(<PasswordChangePage />);
    await user.type(screen.getByLabelText('当前密码'), 'uncommon current password');
    await user.click(screen.getByRole('button', { name: '确认当前密码' }));
    await screen.findByText('当前密码已确认');
    const password = screen.getByLabelText<HTMLInputElement>('新密码');
    await user.type(password, 'uncommon replacement password');
    await user.click(screen.getByRole('button', { name: '修改密码' }));
    expect((await screen.findByRole('alert')).textContent).toContain('请先完成近期重新认证');
    expect(password.value).toBe('');
    expect(password.disabled).toBe(true);
    expect(request).toHaveBeenCalledTimes(2);
  });
});
