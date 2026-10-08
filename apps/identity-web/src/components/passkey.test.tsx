import { render, screen, cleanup } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { api } from '../lib/api';
import { PasskeyReauth } from './PasskeyReauth';

afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
it('用户取消Passkey时不发送验证请求或制造强认证成功', async () => {
  const user = userEvent.setup();
  vi.stubGlobal('PublicKeyCredential', function FakePublicKeyCredential() { return {}; });
  const get = vi.fn().mockRejectedValue(new DOMException('User cancelled', 'NotAllowedError'));
  const original = navigator.credentials;
  Object.defineProperty(navigator, 'credentials', { configurable: true, value: { create: vi.fn(), get } });
  try {
    const request = vi.spyOn(api, 'request').mockResolvedValue({ challenge_id: '65d69320-97e8-4de0-a062-4c0f948a1b80', purpose: 'passkey_reauthentication', expires_at: new Date(Date.now() + 300_000).toISOString(), publicKey: { challenge: 'AQID', rpId: 'localhost', userVerification: 'required' } });
    const confirmed = vi.fn();
    render(<PasskeyReauth onConfirmed={confirmed} />);
    await user.click(screen.getByRole('button', { name: '使用Passkey确认身份' }));
    expect((await screen.findByRole('alert')).textContent).toContain('已取消');
    expect(confirmed).not.toHaveBeenCalled();
    expect(request).toHaveBeenCalledTimes(1);
    expect(get).toHaveBeenCalledTimes(1);
  } finally { Object.defineProperty(navigator, 'credentials', { configurable: true, value: original }); }
});
