import { useEffect, useRef, useState } from 'react';
import { ApiError } from '../lib/api';
import { authenticatePasskey, passkeyFailure } from '../lib/passkeyAuth';
import { webauthnAvailable } from '../lib/webauthn';
import { useRetryAfter } from '../lib/auth-feedback';
import { Button } from './Button';
import { Status } from './Status';

export function PasskeyReauth({ onConfirmed }: { onConfirmed: () => void | Promise<void> }) {
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [apiFailure,setApiFailure]=useState<ApiError|null>(null);
  const active=useRef<AbortController|null>(null);
  const retry=useRetryAfter();
  useEffect(()=>()=>active.current?.abort(),[]);
  async function verify() {
    if(active.current!==null||retry.remaining>0)return;
    const controller=new AbortController();active.current=controller;
    setBusy(true); setFailure(null); setApiFailure(null);
    try { await authenticatePasskey('reauthentication',controller.signal);if(controller.signal.aborted)return;await onConfirmed(); } catch (error) {if(controller.signal.aborted)return;const detail=passkeyFailure(error);retry.start(detail);setApiFailure(detail);setFailure(detail.message);} finally {active.current=null;setBusy(false);}
  }
  return <><Button disabled={!webauthnAvailable()||retry.remaining>0} loading={busy} loadingLabel="正在验证Passkey…" onClick={() => { void verify(); }}>{retry.remaining>0?`${String(retry.remaining)}秒后可重新确认`:'使用Passkey确认身份'}</Button>{failure && <Status kind={apiFailure?.code==='CLIENT_CANCELLED'?'error':apiFailure?.status===429?'limited':apiFailure?.status===503||apiFailure?.status===0?'unavailable':'error'} title="身份确认未完成" description={failure} requestId={apiFailure?.requestId} />}</>;
}
