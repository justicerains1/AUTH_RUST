import * as Dialog from '@radix-ui/react-dialog';
import { useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { asApiError, useRetryAfter } from '../lib/auth-feedback';
import { Button } from './Button';
import { RequestFeedback } from './RequestFeedback';
import type { ApiError } from '../lib/api';

type Props = { trigger: ReactNode; title: string; description: string; confirmLabel: string; onConfirm: () => Promise<void>; onReauthentication?: (error: ApiError) => void };
export function AccountAction({ trigger, title, description, confirmLabel, onConfirm, onReauthentication }: Props) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<ApiError | null>(null);
  const active = useRef(false);
  const cancel = useRef<HTMLButtonElement>(null);
  const retry = useRetryAfter();
  async function confirm() {
    if (active.current || retry.remaining > 0) return;
    active.current = true; setBusy(true); setFailure(null);
    try { await onConfirm(); setOpen(false); }
    catch (error) {
      const detail = asApiError(error);
      if (detail.code === 'AUTH_REAUTH_REQUIRED' && detail.next !== undefined && onReauthentication !== undefined) { setOpen(false); onReauthentication(detail); }
      else { setFailure(detail); retry.start(detail); }
    } finally { active.current = false; setBusy(false); }
  }
  return <Dialog.Root open={open} onOpenChange={(value) => { if (!active.current) { setOpen(value); setFailure(null); } }}><Dialog.Trigger asChild>{trigger}</Dialog.Trigger><Dialog.Portal><Dialog.Overlay className="dialog-overlay" /><Dialog.Content className="dialog-content" onOpenAutoFocus={(event) => { event.preventDefault(); cancel.current?.focus(); }} onEscapeKeyDown={(event) => { if (busy) event.preventDefault(); }} onPointerDownOutside={(event) => { if (busy) event.preventDefault(); }}><Dialog.Title>{title}</Dialog.Title><Dialog.Description>{description}</Dialog.Description><RequestFeedback error={failure} title="操作未完成" remaining={retry.remaining} /><div className="actions dialog-actions"><Dialog.Close asChild><Button ref={cancel} disabled={busy}>取消</Button></Dialog.Close><Button variant="danger" loading={busy} disabled={retry.remaining > 0} loadingLabel="正在处理…" onClick={() => { void confirm(); }}>{confirmLabel}</Button></div></Dialog.Content></Dialog.Portal></Dialog.Root>;
}
