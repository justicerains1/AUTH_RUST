import { Button } from './Button';

type CursorPaginationProps = { hasPrevious: boolean; hasNext: boolean; loading?: boolean; onPrevious: () => void; onNext: () => void; label?: string; };
export function CursorPagination({ hasPrevious, hasNext, loading = false, onPrevious, onNext, label = '列表分页' }: CursorPaginationProps) {
  return <nav className="pagination" aria-label={label}><Button disabled={!hasPrevious || loading} onClick={onPrevious}>上一页</Button><Button disabled={!hasNext || loading} onClick={onNext}>下一页</Button>{loading && <span role="status">正在加载列表…</span>}</nav>;
}
