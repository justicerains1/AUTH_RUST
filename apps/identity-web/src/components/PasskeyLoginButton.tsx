import { useEffect, useRef, useState } from 'react';
import type { PasswordLogin } from '../lib/api';
import { authenticatePasskey, passkeyFailure } from '../lib/passkeyAuth';
import { webauthnAvailable } from '../lib/webauthn';
import { useRetryAfter } from '../lib/auth-feedback';
import { Button } from './Button';
import { Status } from './Status';

type Authenticated = Extract<PasswordLogin, { status: 'authenticated' }>;
export function PasskeyLoginButton({ disabled = false, onAuthenticated, onPasswordFallback, onBusyChange }: { disabled?: boolean; onAuthenticated: (result: Authenticated) => void | Promise<void>; onPasswordFallback?: () => void; onBusyChange?: (busy: boolean) => void }) {
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<ReturnType<typeof passkeyFailure> | null>(null);
  const active = useRef<AbortController | null>(null);
  const retry = useRetryAfter();
  useEffect(() => () => active.current?.abort(), []);
  async function login() {
    if (active.current !== null || disabled || retry.remaining > 0) return;
    const controller = new AbortController(); active.current = controller; setBusy(true); onBusyChange?.(true); setFailure(null);
    try {
      const result = await authenticatePasskey('login', controller.signal);
      if (controller.signal.aborted) return;
      if (result.status !== 'authenticated') throw passkeyFailure(new Error('Unexpected authentication result'));
      await onAuthenticated(result);
    } catch (error) { if (!controller.signal.aborted) { const failure = passkeyFailure(error); setFailure(failure); retry.start(failure); } }
    finally { active.current = null; setBusy(false); onBusyChange?.(false); }
  }
  const available = webauthnAvailable();
  return <div className="passkey-login"><Button disabled={disabled || !available || retry.remaining > 0} loading={busy} loadingLabel="正在验证通行密钥…" onClick={() => { void login(); }}>{retry.remaining > 0 ? `${String(retry.remaining)}秒后可重试通行密钥` : '使用通行密钥登录'}</Button>
    {!available && <p className="muted public-auth-note">当前浏览器暂不支持通行密钥，可继续使用密码登录。</p>}
    {busy && <Button onClick={() => { active.current?.abort(); setFailure(passkeyFailure(new DOMException('Cancelled', 'AbortError'))); }}>取消通行密钥验证</Button>}
    {failure && <Status kind={failure.code === 'CLIENT_CANCELLED' ? 'error' : failure.status === 429 ? 'limited' : failure.status === 503 || failure.status === 0 ? 'unavailable' : 'error'} title={failure.code === 'CLIENT_CANCELLED' ? '通行密钥验证已取消' : '通行密钥登录未完成'} description={failure.message} requestId={failure.requestId} />}
    {failure && onPasswordFallback && <Button onClick={onPasswordFallback}>改用密码登录</Button>}
  </div>;
}
