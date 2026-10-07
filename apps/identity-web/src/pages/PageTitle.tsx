import { useEffect } from 'react';
import { zhCN } from '../lib/i18n';

export function PageTitle({ title }: { title: string }) {
  useEffect(() => { document.title = `${title} · ${zhCN.brand}`; }, [title]);
  return null;
}
