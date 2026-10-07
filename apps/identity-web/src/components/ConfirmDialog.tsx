import * as Dialog from '@radix-ui/react-dialog';
import { useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { Button } from './Button';
import { Status } from './Status';

type ConfirmDialogProps = { trigger: ReactNode; title: string; description: string; confirmLabel: string; onConfirm: () => void | Promise<void>; };

export function ConfirmDialog({ trigger, title, description, confirmLabel, onConfirm }: ConfirmDialogProps) {
  const [open, setOpen] = useState(false);
  const [pending, setPending] = useState(false);
  const [failed, setFailed] = useState(false);
  const cancelRef = useRef<HTMLButtonElement>(null);
  async function confirm() {
    setPending(true);
    setFailed(false);
    try { await onConfirm(); setOpen(false); } catch { setFailed(true); } finally { setPending(false); }
  }
  return <Dialog.Root open={open} onOpenChange={(next) => { if (!pending) { setOpen(next); setFailed(false); } }}>
    <Dialog.Trigger asChild>{trigger}</Dialog.Trigger>
    <Dialog.Portal><Dialog.Overlay className="dialog-overlay" /><Dialog.Content className="dialog-content" onOpenAutoFocus={(event) => { event.preventDefault(); cancelRef.current?.focus(); }} onEscapeKeyDown={(event) => { if (pending) event.preventDefault(); }} onPointerDownOutside={(event) => { if (pending) event.preventDefault(); }}>
      <Dialog.Title>{title}</Dialog.Title><Dialog.Description>{description}</Dialog.Description>
      {failed && <Status kind="error" title="操作暂未完成" description="请重试；此次没有确认完成。" />}
      <div className="actions dialog-actions"><Dialog.Close asChild><Button ref={cancelRef} disabled={pending}>取消</Button></Dialog.Close><Button variant="danger" loading={pending} loadingLabel="正在处理…" onClick={() => { void confirm(); }}>{confirmLabel}</Button></div>
    </Dialog.Content></Dialog.Portal>
  </Dialog.Root>;
}
