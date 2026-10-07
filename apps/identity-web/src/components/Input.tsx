import * as Label from '@radix-ui/react-label';
import { useId } from 'react';
import type { InputHTMLAttributes, Ref } from 'react';
import { FieldError } from './FieldError';

type InputProps = InputHTMLAttributes<HTMLInputElement> & { label: string; error?: string | undefined; hint?: string; ref?: Ref<HTMLInputElement>; };

export function Input({ id: suppliedId, label, error, hint, className = '', ...props }: InputProps) {
  const generatedId = useId();
  const id = suppliedId ?? generatedId;
  const description = [hint ? `${id}-hint` : undefined, error ? `${id}-error` : undefined, props['aria-describedby']].filter(Boolean).join(' ') || undefined;
  return (
    <div className="field">
      <Label.Root htmlFor={id}>{label}</Label.Root>
      <input {...props} id={id} className={`input ${className}`} aria-invalid={Boolean(error) || undefined} aria-describedby={description} />
      {hint && <p className="field-hint" id={`${id}-hint`}>{hint}</p>}
      <FieldError id={`${id}-error`} message={error} />
    </div>
  );
}
