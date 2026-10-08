import * as Dialog from '@radix-ui/react-dialog';
import { useEffect, useRef, useState } from 'react';
import { api, ApiError, passwordReauthSchema } from '../lib/api';
import { useRetryAfter, asApiError } from '../lib/auth-feedback';
import { Button } from './Button';
import { Password } from './Password';
import { FactorChallenge } from './FactorChallenge';
import { PasskeyReauth } from './PasskeyReauth';
import { RequestFeedback } from './RequestFeedback';

type Method = 'password' | 'totp' | 'passkey' | 'recovery_code';
type Challenge = { challenge_id: string; methods: ('totp' | 'recovery_code')[]; expires_at: string };
export function ReauthenticationDialog({ open, onOpenChange, requiredStrength, methods = ['password', 'totp', 'passkey', 'recovery_code'], onConfirmed }: { open: boolean; onOpenChange: (open: boolean) => void; requiredStrength: 'password' | 'strong'; methods?: readonly Method[]; onConfirmed: () => void | Promise<void> }) {
  const [password, setPassword] = useState(''); const [challenge, setChallenge] = useState<Challenge | null>(null);
  const [busy, setBusy] = useState(false); const [failure, setFailure] = useState<ApiError | null>(null); const cancel = useRef<HTMLButtonElement>(null); const active = useRef(false); const retry = useRetryAfter();
  const [factorBusy, setFactorBusy] = useState(false); const [passkeyBusy, setPasskeyBusy] = useState(false);
  const request = useRef<AbortController | null>(null); const opener = useRef<HTMLElement | null>(null);
  const passwordInput = useRef<HTMLInputElement>(null);
  useEffect(() => () => request.current?.abort(), []);
  function clearPassword() { if (passwordInput.current) passwordInput.current.value = ''; setPassword(''); }
  function close() { request.current?.abort(); clearPassword(); setChallenge(null); setFailure(null); onOpenChange(false); }
  async function complete() { clearPassword(); setChallenge(null); await onConfirmed(); onOpenChange(false); }
  async function confirmPassword() {
    if (active.current || factorBusy || passkeyBusy || retry.remaining > 0 || password.length === 0) return;
    active.current = true; setBusy(true); setFailure(null);
    const controller = new AbortController(); request.current = controller;
    try { const result = await api.request('/me/reauth/password', passwordReauthSchema, { method: 'POST', body: { password }, signal: controller.signal }); if (controller.signal.aborted) return; if (result.status === 'mfa_required') setChallenge(result); else if (requiredStrength === 'strong' && !result.strong) throw new ApiError(403, 'AUTH_REAUTH_REQUIRED', '此操作需要通行密钥或第二因素验证'); else await complete(); } catch (error) { if (controller.signal.aborted) return; const detail = asApiError(error); retry.start(detail); setFailure(detail); } finally { clearPassword(); active.current = false; request.current = null; setBusy(false); }
  }
  return <Dialog.Root open={open} onOpenChange={(next) => { if (!next) close(); }}><Dialog.Portal><Dialog.Overlay className="dialog-overlay" /><Dialog.Content className="dialog-content" onOpenAutoFocus={(event) => { opener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null; event.preventDefault(); cancel.current?.focus(); }} onCloseAutoFocus={(event) => { event.preventDefault(); opener.current?.focus(); }}>
    <Dialog.Title>确认当前身份</Dialog.Title><Dialog.Description>{requiredStrength === 'strong' ? '请完成通行密钥或密码加第二因素验证。完成后请再次确认原操作。' : '请确认当前密码，完成后可继续原操作。'}</Dialog.Description>
    {challenge ? <FactorChallenge key={challenge.challenge_id} challenge={challenge} purpose="reauthentication" onBusyChange={setFactorBusy} onComplete={async (result) => { if (result.status !== 'reauthenticated' || requiredStrength === 'strong' && !result.strong) throw new ApiError(403, 'AUTH_REAUTH_REQUIRED', '仍需要合格的强认证'); await complete(); }} onRestart={() => { setChallenge(null); setFailure(null); }} /> : <form onSubmit={(event) => { event.preventDefault(); void confirmPassword(); }}>
      <Password ref={passwordInput} label="当前密码" autoComplete="current-password" value={password} onChange={(event) => { setPassword(event.target.value); }} disabled={busy || passkeyBusy} /><Button type="submit" variant="primary" loading={busy} disabled={password.length === 0 || retry.remaining > 0 || passkeyBusy} loadingLabel="正在确认…">{retry.remaining > 0 ? `${String(retry.remaining)}秒后可重试` : '确认密码'}</Button>
    </form>}
    {methods.includes('passkey') && !busy && !challenge && <PasskeyReauth disabled={factorBusy} onBusyChange={setPasskeyBusy} onConfirmed={complete} />}
    <RequestFeedback error={failure} title="身份确认未完成" remaining={retry.remaining} />
    <Button ref={cancel} onClick={close}>取消身份确认</Button>
  </Dialog.Content></Dialog.Portal></Dialog.Root>;
}
