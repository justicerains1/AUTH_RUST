import * as Dialog from '@radix-ui/react-dialog';
import { useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { ApiError } from '../../lib/api';
import { asApiError } from '../../lib/auth-feedback';
import { Status } from '../../components/Status';
import { Table } from '../../components/Table';
import { useRetryAfter } from '../../lib/auth-feedback';
import { ReauthenticationDialog } from '../../components/ReauthenticationDialog';
import { Button } from '../../components/Button';

export function AdminFailure({ error, retry }: { error: unknown; retry?: () => void }) { const detail = error instanceof ApiError ? error : null; return <Status kind={detail?.status === 403 ? 'error' : 'unavailable'} title={detail?.status === 403 ? '当前账号不能访问此后台数据' : '暂时无法加载'} description={detail?.message ?? '请重新加载后再试。'} requestId={detail?.requestId} onRetry={detail?.status === 403 ? undefined : retry} />; }
export function AdminAction({ label, title, description, action, onComplete, danger = false }: { label: string; title: string; description: string; action: () => Promise<unknown>; onComplete: (opener?: HTMLElement) => void | Promise<void>; danger?: boolean }) {
  const [failure, setFailure] = useState<ApiError | null>(null); const [open, setOpen] = useState(false); const [reauth, setReauth] = useState(false); const [pending, setPending] = useState(false); const [confirmed, setConfirmed] = useState(false); const active = useRef(false); const cancel = useRef<HTMLButtonElement>(null); const retry = useRetryAfter(); const opener = useRef<HTMLElement | null>(null);
  async function execute() {
    if (active.current || retry.remaining > 0) return;
    active.current = true; setPending(true); setFailure(null);
    try { await action(); setOpen(false); setConfirmed(false); await onComplete(opener.current ?? undefined); }
    catch (error) { const detail = asApiError(error); retry.start(detail); setFailure(detail); if (detail.next?.status === 'reauth_required') { setOpen(false); setConfirmed(false); setReauth(true); } }
    finally { active.current = false; setPending(false); }
  }
  return <><Button disabled={retry.remaining > 0} variant={danger ? 'danger' : 'secondary'} onClick={(event) => { opener.current = event.currentTarget; setOpen(true); setFailure(null); }}>{retry.remaining > 0 ? `${String(retry.remaining)}秒后可重试` : label}</Button>
    <Dialog.Root open={open} onOpenChange={(next) => { if (!pending) setOpen(next); }}><Dialog.Portal><Dialog.Overlay className="dialog-overlay" /><Dialog.Content className="dialog-content" onOpenAutoFocus={(event) => { event.preventDefault(); cancel.current?.focus(); }} onCloseAutoFocus={(event) => { event.preventDefault(); opener.current?.focus(); }} onEscapeKeyDown={(event) => { if (pending) event.preventDefault(); }} onPointerDownOutside={(event) => { if (pending) event.preventDefault(); }}>
      <Dialog.Title>{title}</Dialog.Title><Dialog.Description>{description}</Dialog.Description>
      {failure && <Status kind={failure.status === 429 ? 'limited' : failure.status === 503 || failure.status === 0 ? 'unavailable' : 'error'} title="操作未完成" description={failure.code === 'ADMIN_LAST_MEMBER' ? '需要先保留另一位合格管理员，才能完成此操作。' : failure.message} requestId={failure.requestId} />}
      <div className="actions dialog-actions"><Button ref={cancel} disabled={pending} onClick={() => { setOpen(false); }}>取消</Button>{confirmed ? <Button variant={danger ? 'danger' : 'primary'} loading={pending} disabled={retry.remaining > 0} loadingLabel="正在处理…" onClick={() => { void execute(); }}>确认{label}</Button> : <Button onClick={() => { setOpen(false); setReauth(true); }}>确认身份后继续</Button>}</div>
    </Dialog.Content></Dialog.Portal></Dialog.Root>
    <ReauthenticationDialog open={reauth} onOpenChange={setReauth} requiredStrength="strong" {...(failure?.next === undefined ? {} : { methods: failure.next.methods })} onConfirmed={() => { setFailure(null); setConfirmed(true); setReauth(false); setOpen(true); }} />
  </>;
}
export function AdminPanel({ children }: { children: ReactNode }) { return <section className="panel admin-panel">{children}</section>; }
export function AdminTable({ caption, columns, rows }: { caption: string; columns: { key: string; header: string }[]; rows: { id: string; cells: Record<string, ReactNode> }[] }) { return <Table caption={caption} columns={columns.map(({ key, header }) => ({ key, title: header, render: (row: { id: string; cells: Record<string, ReactNode> }) => row.cells[key] }))} rows={rows} rowKey={(row) => row.id} />; }
