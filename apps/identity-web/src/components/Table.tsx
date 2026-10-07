import type { ReactNode } from 'react';
import { Empty } from './Empty';
import { Status } from './Status';

type Column<Row> = { key: string; title: string; render: (row: Row) => ReactNode };
type TableProps<Row> = { caption: string; columns: readonly Column<Row>[]; rows: readonly Row[]; rowKey: (row: Row) => string; loading?: boolean; unavailable?: boolean; onRetry?: (() => void) | undefined; emptyTitle?: string; emptyDescription?: string; };

export function Table<Row>({ caption, columns, rows, rowKey, loading = false, unavailable = false, onRetry, emptyTitle = '暂无记录', emptyDescription = '列表中暂时没有可显示的记录。' }: TableProps<Row>) {
  if (loading) return <Status kind="loading" title={`正在加载${caption}…`} />;
  if (unavailable) return <Status kind="unavailable" title={`暂时无法加载${caption}`} description="请稍后重新加载。" onRetry={onRetry} />;
  if (!rows.length) return <Empty title={emptyTitle} description={emptyDescription} />;
  return <div className="table-container"><table className="table"><caption>{caption}</caption><thead><tr>{columns.map((column) => <th scope="col" key={column.key}>{column.title}</th>)}</tr></thead><tbody>{rows.map((row) => <tr key={rowKey(row)}>{columns.map((column) => <td key={column.key} data-label={column.title}>{column.render(row)}</td>)}</tr>)}</tbody></table></div>;
}
