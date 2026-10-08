import type { ReactNode } from 'react';

type Props = { eyebrow: string; title: ReactNode; description: string; note?: ReactNode; children: ReactNode };
export function PublicAuthLayout({ eyebrow, title, description, note, children }: Props) {
  return <div className="foundation-grid auth-layout"><section className="auth-intro"><p className="eyebrow">{eyebrow}</p><h1>{title}</h1><p className="lead muted">{description}</p>{note && <aside className="security-note">{note}</aside>}</section><section className="panel form-panel auth-panel">{children}</section></div>;
}
