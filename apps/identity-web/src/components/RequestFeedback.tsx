import type { ApiError } from '../lib/api';
import { Status } from './Status';

type Props = { error: ApiError | null; title: string; remaining?: number };
export function RequestFeedback({ error, title, remaining = 0 }: Props) {
  if (error === null) return null;
  const kind = error.status === 429 ? 'limited' : error.status === 0 || error.status === 503 ? 'unavailable' : 'error';
  return <Status kind={kind} title={title} description={remaining > 0 ? `${error.message}。${String(remaining)} 秒后可以重试。` : error.message} requestId={error.requestId} />;
}
