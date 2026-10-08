import { useEffect, useRef, useState } from 'react';
import { useForm } from 'react-hook-form';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, ApiError, meSchema, noContentSchema } from '../lib/api';
import { creationOptions, registrationCredential, registrationOptionsSchema, passkeySchema, passkeyPageSchema, webauthnAvailable } from '../lib/webauthn';
import type { Passkey } from '../lib/webauthn';
import { asApiError, useRetryAfter } from '../lib/auth-feedback';
import { useReauthentication } from '../lib/reauthentication';
import { meQueryKey } from '../lib/query';
import { Button } from '../components/Button';
import { Input } from '../components/Input';
import { Status } from '../components/Status';
import { AccountAction } from '../components/AccountAction';
import { AccountLayout } from '../components/AccountLayout';
import { CursorPagination } from '../components/CursorPagination';
import { Empty } from '../components/Empty';
import { ReauthenticationDialog } from '../components/ReauthenticationDialog';
import { RequestFeedback } from '../components/RequestFeedback';

function validName(name: string) { return name.trim().length > 0 && Array.from(name).length <= 100 && !/[\u0000-\u001f\u007f]/u.test(name); }
export default function PasskeysPage() {
  const client = useQueryClient();
  const identity = useQuery({ queryKey: meQueryKey, queryFn: ({ signal }) => api.request('/me', meSchema, { signal }) });
  const [cursors, setCursors] = useState<string[]>([]);
  const cursor = cursors.at(-1);
  const query = useQuery({ queryKey: ['identity', 'passkeys', cursor ?? ''], queryFn: ({ signal }) => api.request(`/me/passkeys?limit=20${cursor === undefined ? '' : `&cursor=${encodeURIComponent(cursor)}`}`, passkeyPageSchema, { signal }) });
  const reauthentication = useReauthentication(identity.data);
  const retry = useRetryAfter();
  const active = useRef<AbortController | null>(null);
  const [busy, setBusy] = useState(false);
  const [registering, setRegistering] = useState(false);
  const [failure, setFailure] = useState<ApiError | null>(null);
  const [editing, setEditing] = useState<Passkey | null>(null);
  const { register, getValues, resetField, setError, formState: { errors } } = useForm<{ name: string; rename: string }>({ defaultValues: { name: '我的Passkey' } });
  useEffect(() => () => { active.current?.abort(); }, []);
  async function refresh() { await client.invalidateQueries({ queryKey: ['identity', 'passkeys'] }); await client.invalidateQueries({ queryKey: meQueryKey }); }
  function failed(error: unknown) {
    const detail = error instanceof DOMException && ['AbortError', 'NotAllowedError'].includes(error.name) ? new ApiError(0, 'CLIENT_CANCELLED', '通行密钥操作已取消，可以重新开始。') : asApiError(error);
    setFailure(detail); retry.start(detail); if (detail.code === 'AUTH_REAUTH_REQUIRED') reauthentication.request(detail);
  }
  async function create() {
    if (active.current !== null || retry.remaining > 0) return;
    const name = getValues('name'); if (!validName(name)) { setError('name', { message: '请输入1至100个字符的Passkey名称' }); return; }
    const controller = new AbortController(); active.current = controller; setBusy(true); setRegistering(true); setFailure(null);
    try {
      const options = await api.request('/me/passkeys/registration/options', registrationOptionsSchema, { method: 'POST', signal: controller.signal });
      if (Date.parse(options.expires_at) <= Date.now()) throw new ApiError(409, 'AUTH_CHALLENGE_EXPIRED', '本次注册已过期，请重新开始');
      const credential = registrationCredential(await navigator.credentials.create({ publicKey: creationOptions(options.publicKey), signal: controller.signal }));
      if (Date.parse(options.expires_at) <= Date.now()) throw new ApiError(409, 'AUTH_CHALLENGE_EXPIRED', '本次注册已过期，请重新开始');
      await api.request('/me/passkeys/registration/verify', passkeySchema, { method: 'POST', body: { challenge_id: options.challenge_id, name, credential }, signal: controller.signal });
      await refresh();
    } catch (error) { failed(error); } finally { active.current = null; setBusy(false); setRegistering(false); }
  }
  async function rename() {
    if (editing === null || busy || retry.remaining > 0) return;
    const name = getValues('rename'); if (!validName(name)) { setError('rename', { message: '请输入1至100个字符的Passkey名称' }); return; }
    setBusy(true); setFailure(null);
    try { await api.request(`/me/passkeys/${editing.id}`, passkeySchema, { method: 'PATCH', body: { name } }); setEditing(null); await refresh(); }
    catch (error) { failed(error); } finally { setBusy(false); }
  }
  async function remove(id: string) { await api.request(`/me/passkeys/${id}`, noContentSchema, { method: 'DELETE' }); await refresh(); }
  return <AccountLayout title="Passkey管理" description="使用设备的指纹、面容或屏幕锁登录。最多保存十个通行密钥。"><section className="panel"><h2>确认当前身份</h2><p>注册、改名和删除前，服务器会检查所需的近期认证。</p><Button disabled={busy} onClick={() => { reauthentication.request(); }}>确认当前身份</Button>{reauthentication.confirmed && <Status kind="success" title="近期认证已完成" description="请再次确认你要完成的操作。" />}</section><section className="panel"><h2>注册Passkey</h2>{!webauthnAvailable() && <p>当前浏览器不支持Passkey，请使用密码与已有第二因素。</p>}<Input label="Passkey名称" {...register('name')} error={errors.name?.message} /><Button disabled={!reauthentication.confirmed || !webauthnAvailable() || retry.remaining > 0} onClick={() => { void create(); }} loading={busy} loadingLabel="正在注册Passkey…">注册Passkey</Button>{registering && <Button onClick={() => { active.current?.abort(); }}>取消注册</Button>}</section><section className="panel"><h2>已注册Passkey</h2>{query.isPending ? <Status kind="loading" title="正在加载Passkey…" /> : query.isError ? <Status kind="unavailable" title="暂时无法加载Passkey" onRetry={() => { void query.refetch(); }} /> : query.data.items.length === 0 ? <Empty title="还没有注册Passkey" description="确认身份后，注册一个设备支持的通行密钥。" /> : <ul className="credential-list">{query.data.items.map((item) => <li className="detail-row" key={item.id}><div><h3>{item.name}</h3><p>注册时间：{new Date(item.created_at).toLocaleString('zh-CN')}</p><p>最近使用：{item.last_used_at === null ? '尚未使用' : new Date(item.last_used_at).toLocaleString('zh-CN')}</p></div><div className="actions"><Button disabled={busy} onClick={() => { setEditing(item); resetField('rename', { defaultValue: item.name }); }}>编辑名称</Button><AccountAction trigger={<Button disabled={busy} variant="danger">删除Passkey</Button>} title={`删除${item.name}？`} description="此通行密钥将不能再用于登录。删除前请确认仍保有可用的账号保护方式。" confirmLabel="确认删除" onConfirm={() => remove(item.id)} onReauthentication={reauthentication.request} /></div></li>)}</ul>}{editing !== null && <div className="credential-edit"><Input label="新的Passkey名称" {...register('rename')} error={errors.rename?.message} /><div className="actions"><Button loading={busy} disabled={retry.remaining > 0} onClick={() => { void rename(); }}>保存名称</Button><Button disabled={busy} onClick={() => { setEditing(null); }}>取消编辑</Button></div></div>}<CursorPagination hasPrevious={cursors.length > 0} hasNext={Boolean(query.data?.next_cursor)} loading={query.isFetching} onPrevious={() => { setCursors((values) => values.slice(0, -1)); }} onNext={() => { const next = query.data?.next_cursor; if (next !== undefined && next !== null) setCursors((values) => [...values, next]); }} /></section><RequestFeedback error={failure} title="Passkey操作未完成" remaining={retry.remaining} /><ReauthenticationDialog open={reauthentication.open} onOpenChange={reauthentication.setOpen} requiredStrength={reauthentication.requiredStrength} methods={reauthentication.methods} onConfirmed={reauthentication.complete} /></AccountLayout>;
}
