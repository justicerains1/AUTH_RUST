import { useState } from 'react';
import { api, ApiError, mfaVerificationSchema } from '../lib/api';
import { assertionCredential, requestOptions, reauthOptionsSchema, webauthnAvailable } from '../lib/webauthn';
import { Button } from './Button';
import { Status } from './Status';

export function PasskeyReauth({ onConfirmed }: { onConfirmed: () => void | Promise<void> }) {
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [apiFailure,setApiFailure]=useState<ApiError|null>(null);
  async function verify() {
    setBusy(true); setFailure(null); setApiFailure(null);
    try { const options = await api.request('/me/reauth/passkeys/options', reauthOptionsSchema, { method: 'POST' }); const credential = assertionCredential(await navigator.credentials.get({ publicKey: requestOptions(options.publicKey) })); const result = await api.request('/me/reauth/passkeys/verify', mfaVerificationSchema, { method: 'POST', body: { challenge_id: options.challenge_id, credential } }); if (result.status !== 'reauthenticated' || !result.strong) throw new ApiError(0, 'CLIENT_INVALID_RESPONSE', '此结果不能完成强认证'); await onConfirmed(); } catch (error) { setApiFailure(error instanceof ApiError?error:null); setFailure(error instanceof DOMException && ['AbortError', 'NotAllowedError'].includes(error.name) ? 'Passkey验证已取消，你可以重试或使用密码与第二因素。' : error instanceof ApiError ? error.message : 'Passkey验证暂未完成，请重新开始。'); } finally { setBusy(false); }
  }
  return <><Button disabled={!webauthnAvailable()} loading={busy} loadingLabel="正在验证Passkey…" onClick={() => { void verify(); }}>使用Passkey确认身份</Button>{failure && <Status kind={apiFailure?.status===429?'limited':apiFailure?.status===503||apiFailure?.status===0?'unavailable':'error'} title="身份确认未完成" description={failure} requestId={apiFailure?.requestId} />}</>;
}
