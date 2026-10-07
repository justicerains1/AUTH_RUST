import { render, screen, cleanup } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { api, ApiError } from '../lib/api';
import { FactorChallenge } from './FactorChallenge';
import { RecoveryCodes } from './RecoveryCodes';

const challenge = { challenge_id: '65d69320-97e8-4de0-a062-4c0f948a1b80', methods: ['totp', 'recovery_code'] as const, expires_at: '2026-10-08T12:05:00Z' };
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

describe('T08 因素与恢复码行为', () => {
  it('允许粘贴完整验证码，失败清码并保留安全错误', async () => {
    const user = userEvent.setup();
    const request = vi.spyOn(api, 'request').mockRejectedValue(new ApiError(401, 'AUTH_FACTOR_INVALID', '验证码无效'));
    render(<FactorChallenge challenge={challenge} purpose="login" onComplete={() => undefined} onRestart={() => undefined} />);
    const code = screen.getByLabelText<HTMLInputElement>('验证码');
    await user.click(code); await user.paste('123456');
    await user.click(screen.getByRole('button', { name: '验证并登录' }));
    expect((await screen.findByRole('alert')).textContent).toContain('验证码无效');
    expect(code.value).toBe('');
    expect(request.mock.calls[0]?.[0]).toBe('/auth/mfa/totp/verify');
    expect(code.getAttribute('autocomplete')).toBe('one-time-code');
  });

  it('恢复码切换按对应用途端点提交，次数耗尽仅能重新开始', async () => {
    const user = userEvent.setup();
    const request = vi.spyOn(api, 'request').mockRejectedValue(new ApiError(409, 'AUTH_CHALLENGE_CONSUMED', '挑战已失效'));
    render(<FactorChallenge challenge={challenge} purpose="reauthentication" onComplete={() => undefined} onRestart={() => undefined} />);
    await user.click(screen.getByRole('button', { name: '使用恢复码' }));
    await user.type(screen.getByLabelText('恢复码'), 'EXAMPLE_INVALID_RECOVERY_CODE');
    await user.click(screen.getByRole('button', { name: '完成强认证' }));
    expect((await screen.findByRole('alert')).textContent).toContain('挑战已失效');
    expect(request.mock.calls[0]?.[0]).toBe('/auth/mfa/recovery/verify');
    expect(screen.queryByRole('button', { name: '完成强认证' })).toBeNull();
    expect(screen.getByRole('button', { name: '重新开始认证' })).toBeTruthy();
  });

  it('恢复码复制只写用户剪贴板，关闭动作由父组件清除集合', async () => {
    const user = userEvent.setup();
    const copied = vi.spyOn(navigator.clipboard, 'writeText').mockResolvedValue();
    const close = vi.fn();
    const codes = Array.from({ length: 10 }, (_, index) => `EXAMPLE_RECOVERY_${String(index).padStart(8, '0')}`);
    render(<RecoveryCodes codes={codes} onClose={close} />);
    expect(screen.getAllByRole('listitem')).toHaveLength(10);
    await user.click(screen.getByRole('button', { name: '复制恢复码' }));
    expect(copied).toHaveBeenCalledWith(codes.join('\n'));
    await user.click(screen.getByRole('button', { name: '已保存，关闭恢复码' }));
    expect(close).toHaveBeenCalledTimes(1);
    expect(Object.keys(localStorage)).toEqual([]);
    expect(Object.keys(sessionStorage)).toEqual([]);
  });
});
