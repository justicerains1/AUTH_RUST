import { useEffect, useState } from 'react';

type SessionState =
  | { status: 'loading' }
  | { status: 'anonymous' }
  | { status: 'unavailable' }
  | { status: 'authenticated'; subject: string };

async function requestSession(): Promise<SessionState> {
  try {
    const response = await fetch('/bff/session', { credentials: 'same-origin', cache: 'no-store' });
    if (response.status === 401) return { status: 'anonymous' };
    if (!response.ok) return { status: 'unavailable' };
    const value: unknown = await response.json();
    if (
      typeof value !== 'object' || value === null || !('user' in value) ||
      typeof value.user !== 'object' || value.user === null || !('sub' in value.user) ||
      typeof value.user.sub !== 'string'
    ) return { status: 'unavailable' };
    return { status: 'authenticated', subject: value.user.sub };
  } catch {
    return { status: 'unavailable' };
  }
}

export default function InitializationPage() {
  const [session, setSession] = useState<SessionState>({ status: 'loading' });
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState('');

  useEffect(() => {
    document.title = '演示应用 B · 统一身份中心';
    let active = true;
    void requestSession().then((value) => { if (active) setSession(value); });
    return () => { active = false; };
  }, []);

  async function reload() {
    setSession({ status: 'loading' });
    setSession(await requestSession());
  }

  async function logout(identity: boolean) {
    if (busy) return;
    setBusy(true);
    setMessage('');
    try {
      const csrfResponse = await fetch('/bff/csrf', { credentials: 'same-origin', cache: 'no-store' });
      const csrf: unknown = await csrfResponse.json();
      if (
        !csrfResponse.ok || typeof csrf !== 'object' || csrf === null ||
        !('csrf_token' in csrf) || typeof csrf.csrf_token !== 'string'
      ) throw new Error('CSRF unavailable');
      const response = await fetch(identity ? '/bff/identity-logout' : '/bff/logout', {
        method: 'POST', credentials: 'same-origin', cache: 'no-store',
        headers: { 'X-CSRF-Token': csrf.csrf_token },
      });
      if (!response.ok) throw new Error('Logout unavailable');
      const value: unknown = await response.json();
      if (
        typeof value === 'object' && value !== null && 'redirect_to' in value &&
        typeof value.redirect_to === 'string'
      ) {
        window.location.assign(value.redirect_to);
        return;
      }
      setSession({ status: 'anonymous' });
      setMessage(
        typeof value === 'object' && value !== null && 'revocation_status' in value && value.revocation_status === 'failed'
          ? '本应用已退出，平台授权撤销暂未完成。身份平台及其他应用的会话不受影响。'
          : '已退出本应用；身份平台及其他应用的会话不受影响。',
      );
    } catch {
      setMessage('退出暂未完成，请稍后重试。');
    } finally {
      setBusy(false);
    }
  }

  return (
    <main>
      <p className="eyebrow">演示应用 B</p>
      <h1>演示应用 B</h1>
      <p>通过统一身份中心登录，访问本应用。</p>
      {session.status === 'loading' ? <p role="status">正在检查身份…</p>
        : session.status === 'unavailable' ? <>
          <p role="status">身份状态暂不可用，请稍后重试。</p>
          <button type="button" onClick={() => { void reload(); }}>重新检查</button>
        </> : session.status === 'anonymous'
          ? <a className="button" href="/bff/login">通过身份中心登录</a>
          : <section>
            <h2>已登录本应用</h2>
            <p>身份标识：{session.subject}</p>
            <div className="actions">
              <button type="button" disabled={busy} onClick={() => { void logout(false); }}>退出本应用</button>
              <button type="button" disabled={busy} onClick={() => { void logout(true); }}>退出身份平台</button>
            </div>
            <p>本应用退出只清除此应用。平台退出还需要在身份平台确认当前身份会话。</p>
          </section>}
      {message && <p role="status">{message}</p>}
    </main>
  );
}
