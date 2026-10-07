import { describe, expect, it } from 'vitest';
import { creationOptions, decodeBase64Url, encodeBase64Url, registrationOptionsSchema, requestOptions } from './webauthn';

const options = {
  challenge_id: '65d69320-97e8-4de0-a062-4c0f948a1b80', purpose: 'passkey_registration' as const, expires_at: '2026-10-08T12:05:00Z',
  publicKey: { rp: { id: 'localhost', name: '统一身份中心' }, user: { id: 'AQID', name: 'user@example.test', displayName: '用户' }, challenge: 'BAUG', pubKeyCredParams: [{ type: 'public-key' as const, alg: -7 as const }], authenticatorSelection: { residentKey: 'required' as const, requireResidentKey: true as const, userVerification: 'required' as const }, attestation: 'none' as const, excludeCredentials: [{ type: 'public-key' as const, id: 'BwgJ', transports: ['internal' as const] }], extensions: { credProps: true, credentialProtectionPolicy: 'userVerificationRequired' as const, enforceCredentialProtectionPolicy: false, uvm: true } },
};

describe('WebAuthn 二进制与信任字段转换', () => {
  it('二进制字节与base64url双向转换保持每个字节', () => {
    const bytes = new Uint8Array([0, 1, 127, 128, 254, 255]);
    expect(new Uint8Array(decodeBase64Url(encodeBase64Url(bytes.buffer)))).toEqual(bytes);
    expect(() => decodeBase64Url('invalid+padding=')).toThrow();
  });

  it('创建参数转换challenge/user/exclude bytes并保留RP/UV/扩展', () => {
    const validated = registrationOptionsSchema.parse(options);
    const converted = creationOptions(validated.publicKey);
    expect(new Uint8Array(converted.challenge as ArrayBuffer)).toEqual(new Uint8Array([4, 5, 6]));
    expect(new Uint8Array(converted.user.id as ArrayBuffer)).toEqual(new Uint8Array([1, 2, 3]));
    expect(new Uint8Array(converted.excludeCredentials?.[0]?.id as ArrayBuffer)).toEqual(new Uint8Array([7, 8, 9]));
    expect(converted.rp.id).toBe('localhost');
    expect(converted.authenticatorSelection?.userVerification).toBe('required');
    expect(converted.extensions).toMatchObject({ credProps: true, credentialProtectionPolicy: 'userVerificationRequired', uvm: true });
  });

  it('拒绝放松UV和未知option而非静默剥离信任边界字段', () => {
    expect(registrationOptionsSchema.safeParse({ ...options, publicKey: { ...options.publicKey, unexpected: true } }).success).toBe(false);
    expect(registrationOptionsSchema.safeParse({ ...options, publicKey: { ...options.publicKey, authenticatorSelection: { ...options.publicKey.authenticatorSelection, userVerification: 'discouraged' } } }).success).toBe(false);
    expect(requestOptions({ challenge: 'AQID', rpId: 'localhost', userVerification: 'required' }).userVerification).toBe('required');
  });
});
