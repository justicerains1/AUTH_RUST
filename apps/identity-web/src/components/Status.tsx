import { Button } from './Button';

type StatusProps = {
  kind: 'loading' | 'success' | 'error' | 'limited' | 'unavailable';
  title: string;
  description?: string;
  onRetry?: (() => void) | undefined;
  retrying?: boolean;
  requestId?: string | undefined;
};

export function Status({ kind, title, description, onRetry, retrying = false, requestId }: StatusProps) {
  return (
    <div className={`status status--${kind}`} role={kind === 'error' ? 'alert' : 'status'} aria-live={kind === 'error' ? 'assertive' : 'polite'} aria-busy={kind === 'loading' || retrying || undefined}>
      <p className="status-title"><span aria-hidden="true">{kind === 'success' ? '✓' : kind === 'error' ? '！' : '·'}</span>{title}</p>
      {description && <p>{description}</p>}
      {requestId && <p className="request-id">请求编号：{requestId}</p>}
      {onRetry && <Button onClick={onRetry} loading={retrying} loadingLabel="正在重新加载…">重新加载</Button>}
    </div>
  );
}
