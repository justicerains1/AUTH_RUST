import type { ReactNode } from 'react';

export function Empty({ title, description, action }: { title: string; description: string; action?: ReactNode }) {
  return <div className="empty-state"><p className="empty-title">{title}</p><p>{description}</p>{action}</div>;
}
