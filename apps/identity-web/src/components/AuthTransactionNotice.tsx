import type { ReturnTypeOfAuthTransaction } from '../lib/auth-flow';
import { Status } from './Status';

export function AuthTransactionNotice({ flow }: { flow: ReturnTypeOfAuthTransaction }) {
  if (flow.invalid) return <Status kind="error" title="授权请求无效" description="请从应用重新开始登录。" />;
  if (flow.query.isError) return <Status kind="unavailable" title="暂时无法读取授权请求" description="请重试，或返回应用重新开始。" onRetry={() => { void flow.query.refetch(); }} />;
  if (flow.blocked) return <Status kind="loading" title="正在检查应用授权请求…" />;
  if (flow.transaction === undefined) return null;
  return <p className="transaction-note">登录后继续连接 <strong>{flow.transaction.client.name}</strong>。访问范围由你在下一步确认。</p>;
}
