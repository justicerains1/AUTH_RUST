import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { FactorChallenge } from './FactorChallenge';
import { PasskeyLoginButton } from './PasskeyLoginButton';
import { api, ApiError } from '../lib/api';

afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
const challenge = () => ({ challenge_id: '65d69320-97e8-4de0-a062-4c0f948a1b80', methods: ['totp', 'recovery_code'] as const, expires_at: new Date(Date.now() + 300_000).toISOString() });

it('expired MFA offers a fresh start without sending a verification request', async () => {
  const request = vi.spyOn(api, 'request'); const restart = vi.fn();
  render(<FactorChallenge challenge={{ ...challenge(), expires_at: new Date(Date.now() - 1000).toISOString() }} purpose="login" onComplete={vi.fn()} onRestart={restart} />);
  expect(screen.queryByLabelText('验证码')).toBeNull();
  expect(screen.getByRole('alert').textContent).toContain('已过期');
  await userEvent.setup().click(screen.getByRole('button', { name: '重新开始登录' }));
  expect(restart).toHaveBeenCalledOnce(); expect(request).not.toHaveBeenCalled();
});

it('switching from a pasted code clears the secret and Retry-After blocks repeated submissions', async () => {
  const user = userEvent.setup(); const request = vi.spyOn(api, 'request').mockRejectedValue(new ApiError(429, 'RATE_LIMITED', '请求较频繁', { retryAfter: 30 }));
  render(<FactorChallenge challenge={challenge()} purpose="login" onComplete={vi.fn()} onRestart={vi.fn()} />);
  await user.click(screen.getByLabelText('验证码')); await user.paste('123456');
  await user.click(screen.getByRole('button', { name: '使用恢复码' }));
  expect(screen.getByLabelText<HTMLInputElement>('恢复码').value).toBe('');
  await user.click(screen.getByLabelText('恢复码')); await user.paste('RECOVERY_CODE_EXAMPLE_00001');
  await user.click(screen.getByRole('button', { name: '验证并登录' }));
  const blocked = await screen.findByRole('button', { name: /秒后可重试/u }); expect(blocked.hasAttribute('disabled')).toBe(true);
  await user.click(blocked); expect(request).toHaveBeenCalledOnce();
  expect(screen.getByLabelText<HTMLInputElement>('恢复码').value).toBe('');
});

it('unavailable Passkey explains the password fallback without a request', () => {
  vi.stubGlobal('PublicKeyCredential', undefined); const request = vi.spyOn(api, 'request');
  render(<PasskeyLoginButton onAuthenticated={vi.fn()} />);
  expect(screen.getByRole('button', { name: '使用通行密钥登录' }).hasAttribute('disabled')).toBe(true);
  expect(screen.getByText(/当前浏览器暂不支持通行密钥/u)).toBeTruthy(); expect(request).not.toHaveBeenCalled();
});

it('Passkey rate limiting keeps the server cooldown and does not reopen the authenticator', async () => {
  const original = navigator.credentials; vi.stubGlobal('PublicKeyCredential', function PublicKeyCredential() { return {}; });
  const get = vi.fn(); Object.defineProperty(navigator, 'credentials', { configurable: true, value: { create: vi.fn(), get } });
  try {
    const request = vi.spyOn(api, 'request').mockRejectedValue(new ApiError(429, 'RATE_LIMITED', '请稍后再试', { retryAfter: 20 }));
    render(<PasskeyLoginButton onAuthenticated={vi.fn()} />); const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: '使用通行密钥登录' }));
    const retry = await screen.findByRole('button', { name: /秒后可重试通行密钥/u });
    expect(retry.hasAttribute('disabled')).toBe(true); await user.click(retry);
    expect(request).toHaveBeenCalledOnce(); expect(get).not.toHaveBeenCalled();
  } finally { Object.defineProperty(navigator, 'credentials', { configurable: true, value: original }); }
});

it('Passkey cancellation never posts an assertion and keeps an explicit password fallback', async () => {
  const original = navigator.credentials; vi.stubGlobal('PublicKeyCredential', function PublicKeyCredential() { return {}; });
  const get = vi.fn().mockRejectedValue(new DOMException('Cancelled', 'NotAllowedError'));
  Object.defineProperty(navigator, 'credentials', { configurable: true, value: { create: vi.fn(), get } });
  try {
    const request = vi.spyOn(api, 'request').mockResolvedValue({ challenge_id: challenge().challenge_id, purpose: 'passkey_login', expires_at: challenge().expires_at, publicKey: { challenge: 'AQID', rpId: 'localhost', userVerification: 'required' } });
    const fallback = vi.fn(); const completed = vi.fn(); const busy = vi.fn();
    render(<PasskeyLoginButton onAuthenticated={completed} onPasswordFallback={fallback} onBusyChange={busy} />);
    const user = userEvent.setup(); await user.click(screen.getByRole('button', { name: '使用通行密钥登录' }));
    expect((await screen.findByRole('alert')).textContent).toContain('已取消');
    await user.click(screen.getByRole('button', { name: '改用密码登录' }));
    expect(fallback).toHaveBeenCalledOnce(); expect(completed).not.toHaveBeenCalled(); expect(request).toHaveBeenCalledOnce();
    expect(busy.mock.calls).toEqual([[true], [false]]);
  } finally { Object.defineProperty(navigator, 'credentials', { configurable: true, value: original }); }
});
