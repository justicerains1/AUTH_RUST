import { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import type { ApiError, Me } from './api';

export type ReauthenticationMethod = 'password' | 'totp' | 'recovery_code' | 'passkey';
export function factorMethods(identity: Me | undefined): ReauthenticationMethod[] {
  const methods: ReauthenticationMethod[] = ['password'];
  if (identity?.security.totp_enabled) methods.push('totp');
  if ((identity?.security.recovery_codes_remaining ?? 0) > 0) methods.push('recovery_code');
  if ((identity?.security.passkey_count ?? 0) > 0) methods.push('passkey');
  return methods;
}
export function useReauthentication(identity: Me | undefined) {
  const client = useQueryClient();
  const [open, setOpen] = useState(false);
  const [confirmed, setConfirmed] = useState(false);
  const [required, setRequired] = useState<'password' | 'strong' | undefined>();
  const [methods, setMethods] = useState<readonly ReauthenticationMethod[] | undefined>();
  const strength = required ?? (identity !== undefined && (identity.security.totp_enabled || identity.security.passkey_count > 0) ? 'strong' : 'password');
  function request(error?: ApiError) {
    setRequired(error?.next?.required_strength);
    setMethods(error?.next?.methods);
    setConfirmed(false);
    setOpen(true);
  }
  async function complete() {
    setConfirmed(true);
    await client.invalidateQueries({ queryKey: ['identity', 'me'] });
  }
  return { open, setOpen, confirmed, request, complete, requiredStrength: strength, methods: methods ?? factorMethods(identity) };
}
