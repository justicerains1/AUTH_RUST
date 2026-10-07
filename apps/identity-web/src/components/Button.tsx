import type { ButtonHTMLAttributes, Ref } from 'react';

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: 'primary' | 'secondary' | 'danger';
  loading?: boolean;
  loadingLabel?: string;
  ref?: Ref<HTMLButtonElement>;
};

export function Button({ variant = 'secondary', loading = false, loadingLabel = '正在处理…', disabled, children, className = '', type = 'button', ...props }: ButtonProps) {
  return (
    <button {...props} type={type} disabled={disabled === true || loading} aria-busy={loading || undefined} className={`button button--${variant} ${className}`}>
      {loading ? loadingLabel : children}
    </button>
  );
}
