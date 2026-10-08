import { useEffect, useState } from 'react';
import { ApiError } from './api';

export function asApiError(error: unknown): ApiError {
  return error instanceof ApiError ? error : new ApiError(0, 'CLIENT_NETWORK_UNAVAILABLE', '网络暂不可用，请重试');
}

/** Count only the server Retry-After window; reaching zero never replays a request. */
export function useRetryAfter() {
  const [deadline, setDeadline] = useState(0);
  const [remaining, setRemaining] = useState(0);
  useEffect(() => {
    if (deadline === 0) return;
    const interval = window.setInterval(() => {
      const seconds = Math.max(0, Math.ceil((deadline - Date.now()) / 1000));
      setRemaining(seconds);
      if (seconds === 0) window.clearInterval(interval);
    }, 200);
    return () => { window.clearInterval(interval); };
  }, [deadline]);
  function start(error: ApiError) {
    const seconds = error.status === 429 && error.retryAfter !== undefined && Number.isSafeInteger(error.retryAfter) && error.retryAfter > 0 ? error.retryAfter : 0;
    setDeadline(seconds === 0 ? 0 : Date.now() + seconds * 1000);
    setRemaining(seconds);
  }
  return { remaining, start };
}
