import * as Label from '@radix-ui/react-label';
import { useId, useState } from 'react';
import type { InputHTMLAttributes, Ref } from 'react';
import { FieldError } from './FieldError';

type PasswordProps = Omit<InputHTMLAttributes<HTMLInputElement>, 'type'> & { label: string; error?: string | undefined; ref?: Ref<HTMLInputElement>; };

export function Password({ id: suppliedId, label, error, autoComplete = 'current-password', ...props }: PasswordProps) {
  const generatedId = useId();
  const id = suppliedId ?? generatedId;
  const [visible, setVisible] = useState(false);
  return (
    <div className="field">
      <Label.Root htmlFor={id}>{label}</Label.Root>
      <div className="password-control">
        <input {...props} id={id} type={visible ? 'text' : 'password'} autoComplete={autoComplete} className="input" aria-invalid={Boolean(error) || undefined} aria-describedby={error ? `${id}-error` : props['aria-describedby']} />
        <button type="button" className="password-toggle" aria-pressed={visible} aria-controls={id} aria-label={`${visible ? '隐藏' : '显示'}${label}`} onClick={() => { setVisible((value) => !value); }}>{visible ? '隐藏' : '显示'}</button>
      </div>
      <FieldError id={`${id}-error`} message={error} />
    </div>
  );
}
