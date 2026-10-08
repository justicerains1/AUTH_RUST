import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { api, ApiError } from '../lib/api';
import type { Me } from '../lib/api';
import MfaPage from './MfaPage';
import { RecoveryCodes } from '../components/RecoveryCodes';

const id = '65d69320-97e8-4de0-a062-4c0f948a1b80';
function identity(): Me {
  const now = new Date().toISOString();
  return { user: { id, sub: id, email: 'fixture@example.test', email_verified: true, status: 'active', created_at: now }, session: { id, amr: ['pwd', 'otp'], auth_time: now, strong_at: now, expires_at: new Date(Date.now() + 60_000).toISOString(), created_at: now, current: true }, security: { totp_enabled: true, passkey_count: 0, recovery_codes_remaining: 10, is_admin: false, admin_binding_only: false } };
}
function page(element: React.ReactNode) { const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } }); return render(<QueryClientProvider client={client}><MemoryRouter>{element}</MemoryRouter></QueryClientProvider>); }
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

describe('T18 账号安全操作边界（UI 单元响应）', () => {
  it('过期强认证打开统一对话框，完成身份验证不会自动重放恢复码生成', async () => {
    const view = identity();
    const request = vi.spyOn(api, 'request').mockImplementation((path) => {
      if (path === '/me') return Promise.resolve(view);
      if (path === '/me/mfa/recovery-codes/regenerate') return Promise.reject(new ApiError(403, 'AUTH_REAUTH_REQUIRED', '请确认身份', { next: { status: 'reauth_required', required_strength: 'strong', methods: ['totp'] } }));
      if (path === '/me/reauth/password') return Promise.resolve({ status: 'mfa_required', challenge_id: id, purpose: 'reauthentication', methods: ['totp'], expires_at: new Date(Date.now() + 60_000).toISOString() });
      return Promise.resolve({ status: 'reauthenticated', reauthenticated_at: new Date().toISOString(), valid_until: new Date(Date.now() + 60_000).toISOString(), strong: true, amr: ['pwd', 'otp'] });
    });
    const user = userEvent.setup(); page(<MfaPage />);
    await screen.findByRole('button', { name: '重新生成恢复码' });
    await user.click(screen.getByRole('button', { name: '重新生成恢复码' }));
    await user.click(screen.getByRole('button', { name: '确认重新生成' }));
    await screen.findByRole('dialog', { name: '确认当前身份' });
    await user.type(screen.getByLabelText('当前密码'), 'fixture current password');
    await user.click(screen.getByRole('button', { name: '确认密码' }));
    await user.type(await screen.findByLabelText('验证码'), '123456');
    await user.click(screen.getByRole('button', { name: '完成强认证' }));
    await waitFor(() => { expect(screen.queryByRole('dialog')).toBeNull(); });
    expect(request.mock.calls.filter(([path]) => path === '/me/mfa/recovery-codes/regenerate')).toHaveLength(1);
    expect(screen.queryByRole('region', { name: '一次性恢复码' })).toBeNull();
    expect(await screen.findByText('近期认证已完成')).toBeTruthy();
  });
  it('恢复码仅在服务端生成成功后展示，关闭后移出页面', async () => {
    const codes = Array.from({ length: 10 }, (_, index) => `recovery-fixture-${String(index).padStart(2, '0')}-value`);
    vi.spyOn(api, 'request').mockImplementation((path) => path === '/me' ? Promise.resolve(identity()) : Promise.resolve({ codes, shown_once: true }));
    const user = userEvent.setup(); page(<MfaPage />);
    await screen.findByRole('button', { name: '重新生成恢复码' });
    expect(screen.queryByRole('region', { name: '一次性恢复码' })).toBeNull();
    await user.click(screen.getByRole('button', { name: '重新生成恢复码' }));
    await user.click(screen.getByRole('button', { name: '确认重新生成' }));
    expect(await screen.findByText(codes[0] ?? '')).toBeTruthy();
    await user.click(screen.getByRole('button', { name: '已保存，关闭恢复码' }));
    expect(screen.queryByText(codes[0] ?? '')).toBeNull();
  });
  it('TOTP绑定到期清除设置密钥与验证码，且不自动提交确认请求', async () => {
    const view = identity(); view.security.totp_enabled = false; view.security.recovery_codes_remaining = 0;
    const request = vi.spyOn(api, 'request').mockImplementation((path) => {
      if (path === '/me') return Promise.resolve(view);
      if (path === '/me/reauth/password') return Promise.resolve({ status: 'reauthenticated', strong: false });
      return Promise.resolve({ challenge_id: id, purpose: 'totp_enrollment', secret: 'UI_ONLY_SETUP_SECRET', otpauth_uri: 'otpauth://totp/fixture?secret=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA', expires_at: new Date(Date.now() + 250).toISOString() });
    });
    const user = userEvent.setup(); page(<MfaPage />);
    await screen.findByRole('button', { name: '确认当前身份' });
    await user.click(screen.getByRole('button', { name: '确认当前身份' }));
    await user.type(screen.getByLabelText('当前密码'), 'fixture current password');
    await user.click(screen.getByRole('button', { name: '确认密码' }));
    await screen.findByText('近期认证已完成');
    await user.click(screen.getByRole('button', { name: '开始绑定TOTP' }));
    await screen.findByText('UI_ONLY_SETUP_SECRET');
    await user.type(screen.getByLabelText('验证码'), '123456');
    await screen.findByText('绑定已过期');
    expect(screen.queryByText('UI_ONLY_SETUP_SECRET')).toBeNull();
    expect(screen.queryByLabelText('验证码')).toBeNull();
    expect(request.mock.calls.some(([path]) => path === '/me/mfa/totp/enrollment/confirm')).toBe(false);
  });
  it('恢复码下载只由明确点击触发，并立即回收下载URL', async () => {
    const createObjectURL = vi.fn(() => 'blob:fixture'); const revokeObjectURL = vi.fn();
    vi.stubGlobal('URL', Object.assign(URL, { createObjectURL, revokeObjectURL }));
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => {});
    const user = userEvent.setup(); page(<RecoveryCodes codes={['ui-fixture-recovery']} onClose={() => {}} />);
    expect(createObjectURL).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: '下载恢复码' }));
    expect(createObjectURL).toHaveBeenCalledTimes(1); expect(click).toHaveBeenCalledTimes(1);
    expect(revokeObjectURL).toHaveBeenCalledWith('blob:fixture');
  });
});
