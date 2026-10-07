import { useState } from 'react';
import { useForm } from 'react-hook-form';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router';
import { api, ApiError, meSchema, noContentSchema, passwordReauthSchema } from '../lib/api';
import { creationOptions, registrationCredential, registrationOptionsSchema, passkeySchema, passkeyPageSchema, webauthnAvailable } from '../lib/webauthn';
import type { Passkey } from '../lib/webauthn';
import { meQueryKey } from '../lib/query';
import { Button } from '../components/Button';
import { Input } from '../components/Input';
import { Password } from '../components/Password';
import { Status } from '../components/Status';
import { ConfirmDialog } from '../components/ConfirmDialog';
import { PasskeyReauth } from '../components/PasskeyReauth';
import { FactorChallenge } from '../components/FactorChallenge';
import { PageTitle } from './PageTitle';

type Challenge = { challenge_id: string; methods: ('totp' | 'recovery_code')[]; expires_at: string };
export default function PasskeysPage() {
  const client = useQueryClient();
  const identity = useQuery({ queryKey: meQueryKey, queryFn: ({ signal }) => api.request('/me', meSchema, { signal }) });
  const query = useQuery({ queryKey: ['identity', 'passkeys'], queryFn: ({ signal }) => api.request('/me/passkeys?limit=20', passkeyPageSchema, { signal }) });
  const [confirmed, setConfirmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [apiFailure,setApiFailure]=useState<ApiError|null>(null);
  const [challenge, setChallenge] = useState<Challenge | null>(null);
  const [editing, setEditing] = useState<Passkey | null>(null);
  const { register, getValues, resetField } = useForm<{ name: string; password: string; rename: string }>({ defaultValues: { name: '我的Passkey' } });
  async function refresh() { await client.invalidateQueries({ queryKey: ['identity'] }); }
  async function passwordConfirm() { setBusy(true); setFailure(null); setApiFailure(null); try { const response = await api.request('/me/reauth/password', passwordReauthSchema, { method: 'POST', body: { password: getValues('password') } }); if (response.status === 'mfa_required') setChallenge(response); else setConfirmed(true); } catch (error) { failed(error); } finally { resetField('password'); setBusy(false); } }
  async function create() {
    const name = getValues('name'); if (!name.trim() || name.length > 100 || /[\u0000-\u001f\u007f]/u.test(name)) { setFailure('请输入1至100个字符的Passkey名称'); return; }
    setBusy(true); setFailure(null); setApiFailure(null);
    try { const options = await api.request('/me/passkeys/registration/options', registrationOptionsSchema, { method: 'POST' }); const credential = registrationCredential(await navigator.credentials.create({ publicKey: creationOptions(options.publicKey) })); await api.request('/me/passkeys/registration/verify', passkeySchema, { method: 'POST', body: { challenge_id: options.challenge_id, name, credential } }); await refresh(); } catch (error) { failed(error); } finally { setBusy(false); }
  }
  async function rename() { if (!editing) return; const name = getValues('rename'); if (!name.trim() || name.length > 100) { setFailure('请输入1至100个字符的Passkey名称'); return; } try { await api.request(`/me/passkeys/${editing.id}`, passkeySchema, { method: 'PATCH', body: { name } }); setEditing(null); await refresh(); } catch (error) { failed(error); } }
  async function remove(id: string) { try { await api.request(`/me/passkeys/${id}`, noContentSchema, { method: 'DELETE' }); await refresh(); } catch (error) { failed(error); throw error; } }
  function failed(error: unknown) { setApiFailure(error instanceof ApiError?error:null); setFailure(error instanceof DOMException && ['AbortError', 'NotAllowedError'].includes(error.name) ? 'Passkey操作已取消，账号没有完成此操作。你可以重试或使用密码登录。' : error instanceof ApiError ? error.message : 'Passkey操作暂未完成，请重新开始。'); }
  return <><PageTitle title="Passkey管理" /><p className="eyebrow">ACCOUNT / PASSKEYS</p><h1>Passkey管理</h1><p><Link to="/me">返回账号安全</Link></p><p className="muted">通过设备验证登录；同步Passkey也受服务器挑战和用户验证检查。最多保存十个。</p>{failure && <Status kind={apiFailure?.status===429?'limited':apiFailure?.status===503||apiFailure?.status===0?'unavailable':'error'} title="Passkey操作未完成" description={failure} requestId={apiFailure?.requestId} />}<div className="component-grid"><section className="panel"><h2>确认身份</h2><Password label="当前密码" {...register('password')} /><Button onClick={() => { void passwordConfirm(); }} loading={busy}>确认当前密码</Button>{(identity.data?.security.passkey_count ?? 0) > 0 && <PasskeyReauth onConfirmed={() => { setConfirmed(true); }} />}{challenge && <FactorChallenge challenge={challenge} purpose="reauthentication" onComplete={(result) => { if (result.status !== 'reauthenticated' || !result.strong) throw new ApiError(0, 'CLIENT_INVALID_RESPONSE', '需要有效强认证'); setConfirmed(true); setChallenge(null); }} onRestart={() => { setChallenge(null); }} />}{confirmed && <Status kind="success" title="近期认证已完成" />}</section><section className="panel"><h2>注册Passkey</h2>{!webauthnAvailable() && <p>当前浏览器不支持Passkey，请使用密码与已有第二因素。</p>}<Input label="Passkey名称" {...register('name')} /><Button disabled={!confirmed || !webauthnAvailable()} onClick={() => { void create(); }} loading={busy} loadingLabel="正在注册Passkey…">注册Passkey</Button></section></div><section className="panel"><h2>已注册Passkey</h2>{query.isPending ? <Status kind="loading" title="正在加载Passkey…" /> : query.isError ? <Status kind="unavailable" title="暂时无法加载Passkey" onRetry={() => { void query.refetch(); }} /> : query.data.items.length === 0 ? <p>还没有注册Passkey。</p> : <ul>{query.data.items.map((item) => <li key={item.id}><strong>{item.name}</strong><div className="actions"><Button onClick={() => { setEditing(item); resetField('rename', { defaultValue: item.name }); }}>编辑名称</Button><ConfirmDialog trigger={<Button variant="danger">删除Passkey</Button>} title={`删除${item.name}？`} description="此Passkey将不能用于登录，需要近期强认证；密码登录方式继续保留。" confirmLabel="确认删除" onConfirm={() => remove(item.id)} /></div></li>)}</ul>}{editing && <><Input label="新的Passkey名称" {...register('rename')} /><Button onClick={() => { void rename(); }}>保存名称</Button><Button onClick={() => { setEditing(null); }}>取消编辑</Button></>}</section></>;
}
