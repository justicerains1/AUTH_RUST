import { useLayoutEffect, useRef, useState } from 'react';

function readFragment(): { token: string | undefined; supplied: boolean } {
  const fragment = window.location.hash;
  const values = new URLSearchParams(fragment.slice(1));
  const tokens = values.getAll('token');
  const token = tokens.length === 1 && Array.from(values.keys()).every((key) => key === 'token') && /^[A-Za-z0-9_-]{43}$/u.test(tokens[0] ?? '') ? tokens[0] : undefined;
  return { token, supplied: fragment.length > 0 };
}

export function useMailAction(lifetimeMinutes: 15 | 30) {
  // The pure read survives StrictMode's repeated render; layout clears the URL before paint.
  const secret = useRef(readFragment());
  const [expiry] = useState(() => Date.now() + lifetimeMinutes * 60_000);
  const [hasToken, setHasToken] = useState(() => readFragment().token !== undefined);
  const [supplied] = useState(() => readFragment().supplied);
  useLayoutEffect(() => {
    if (window.location.hash) window.history.replaceState(window.history.state, '', window.location.pathname + window.location.search);
    const timeout = window.setTimeout(() => { secret.current.token = undefined; setHasToken(false); }, Math.max(0, expiry - Date.now()));
    return () => { window.clearTimeout(timeout); };
  }, [expiry]);
  function value() { return Date.now() < expiry ? secret.current.token : undefined; }
  function clear() { secret.current.token = undefined; setHasToken(false); }
  return { supplied, hasToken, value, clear };
}
