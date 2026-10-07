import { z } from 'zod';

const binary = z.string().min(1).max(87382).regex(/^[A-Za-z0-9_-]+$/u);
const transport = z.enum(['usb', 'nfc', 'ble', 'internal', 'hybrid']);
const descriptor = z.object({ type: z.literal('public-key'), id: binary, transports: z.array(transport).optional() });
const extensionInput = z.strictObject({ credProps: z.boolean().optional(), credentialProtectionPolicy: z.literal('userVerificationRequired').optional(), enforceCredentialProtectionPolicy: z.boolean().optional(), uvm: z.boolean().optional() });
const creation = z.strictObject({
  rp: z.object({ id: z.string().min(1).max(253), name: z.string().min(1).max(100) }),
  user: z.object({ id: binary, name: z.string().min(1).max(254), displayName: z.string().min(1).max(100) }),
  challenge: binary, pubKeyCredParams: z.array(z.object({ type: z.literal('public-key'), alg: z.union([z.literal(-7), z.literal(-257)]) })).min(1),
  timeout: z.number().int().positive().max(300000).optional(), excludeCredentials: z.array(descriptor).max(10).optional(),
  authenticatorSelection: z.object({ residentKey: z.literal('required'), requireResidentKey: z.literal(true), userVerification: z.literal('required') }),
  attestation: z.literal('none'), extensions: extensionInput.optional(),
});
const assertion = z.strictObject({ challenge: binary, rpId: z.string().min(1).max(253), timeout: z.number().int().positive().max(300000).optional(), allowCredentials: z.array(descriptor).max(10).optional(), userVerification: z.literal('required'), extensions: z.strictObject({ uvm: z.boolean().optional() }).optional() });
export const registrationOptionsSchema = z.object({ challenge_id: z.uuid(), purpose: z.literal('passkey_registration'), expires_at: z.iso.datetime({ offset: true }), publicKey: creation });
export const loginOptionsSchema = z.object({ challenge_id: z.uuid(), purpose: z.literal('passkey_login'), expires_at: z.iso.datetime({ offset: true }), publicKey: assertion });
export const reauthOptionsSchema = z.object({ challenge_id: z.uuid(), purpose: z.literal('passkey_reauthentication'), expires_at: z.iso.datetime({ offset: true }), publicKey: assertion });
export const passkeySchema = z.object({ id: z.uuid(), name: z.string().min(1).max(100), created_at: z.iso.datetime({ offset: true }), last_used_at: z.iso.datetime({ offset: true }).nullable() });
export const passkeyPageSchema = z.object({ items: z.array(passkeySchema).max(100), next_cursor: z.string().nullable() });
export type Passkey = z.infer<typeof passkeySchema>;

export function webauthnAvailable(): boolean { return typeof PublicKeyCredential !== 'undefined' && typeof navigator.credentials.create === 'function' && typeof navigator.credentials.get === 'function'; }
export function decodeBase64Url(value: string): ArrayBuffer {
  if (!/^[A-Za-z0-9_-]+$/u.test(value)) throw new TypeError('Invalid credential encoding');
  const text = atob(value.replaceAll('-', '+').replaceAll('_', '/') + '='.repeat((4 - value.length % 4) % 4));
  const bytes = new Uint8Array(text.length);
  for (let i = 0; i < text.length; i += 1) bytes[i] = text.charCodeAt(i);
  return bytes.buffer;
}
export function encodeBase64Url(value: ArrayBuffer): string {
  const bytes = new Uint8Array(value); let text = '';
  for (const byte of bytes) text += String.fromCharCode(byte);
  return btoa(text).replaceAll('+', '-').replaceAll('/', '_').replaceAll('=', '');
}
type RegistrationExtensions = AuthenticationExtensionsClientInputs & { credentialProtectionPolicy?: 'userVerificationRequired'; enforceCredentialProtectionPolicy?: boolean; uvm?: boolean; };
function registrationExtensions(value: z.infer<typeof extensionInput>): RegistrationExtensions { return { ...(value.credProps === undefined ? {} : { credProps: value.credProps }), ...(value.credentialProtectionPolicy === undefined ? {} : { credentialProtectionPolicy: value.credentialProtectionPolicy }), ...(value.enforceCredentialProtectionPolicy === undefined ? {} : { enforceCredentialProtectionPolicy: value.enforceCredentialProtectionPolicy }), ...(value.uvm === undefined ? {} : { uvm: value.uvm }) }; }
function descriptors(values: z.infer<typeof descriptor>[]): PublicKeyCredentialDescriptor[] { return values.map((value) => ({ type: value.type, id: decodeBase64Url(value.id), ...(value.transports === undefined ? {} : { transports: value.transports }) })); }
export function creationOptions(value: z.infer<typeof creation>): PublicKeyCredentialCreationOptions {
  return { rp: value.rp, user: { ...value.user, id: decodeBase64Url(value.user.id) }, challenge: decodeBase64Url(value.challenge), pubKeyCredParams: value.pubKeyCredParams, authenticatorSelection: value.authenticatorSelection, attestation: value.attestation, ...(value.timeout === undefined ? {} : { timeout: value.timeout }), ...(value.excludeCredentials === undefined ? {} : { excludeCredentials: descriptors(value.excludeCredentials) }), ...(value.extensions === undefined ? {} : { extensions: registrationExtensions(value.extensions) }) };
}
export function requestOptions(value: z.infer<typeof assertion>): PublicKeyCredentialRequestOptions { return { challenge: decodeBase64Url(value.challenge), rpId: value.rpId, userVerification: value.userVerification, ...(value.timeout === undefined ? {} : { timeout: value.timeout }), ...(value.allowCredentials === undefined ? {} : { allowCredentials: descriptors(value.allowCredentials) }), ...(value.extensions?.uvm === undefined ? {} : { extensions: { uvm: value.extensions.uvm } as AuthenticationExtensionsClientInputs & { uvm: boolean } }) }; }
function extensionResults(credential: PublicKeyCredential) {
  const result = credential.getClientExtensionResults();
  return { ...(result.credProps === undefined ? {} : { credProps: { rk: result.credProps.rk } }), ...(result.appid === undefined ? {} : { appid: result.appid }) };
}
function common(credential: PublicKeyCredential) { return { id: credential.id, rawId: encodeBase64Url(credential.rawId), type: 'public-key' as const, ...(credential.authenticatorAttachment === 'platform' || credential.authenticatorAttachment === 'cross-platform' ? { authenticatorAttachment: credential.authenticatorAttachment } : {}), clientExtensionResults: extensionResults(credential) }; }
export function registrationCredential(credential: Credential | null) {
  if (!(credential instanceof PublicKeyCredential) || !(credential.response instanceof AuthenticatorAttestationResponse)) throw new TypeError('Unexpected credential result');
  return { ...common(credential), response: { clientDataJSON: encodeBase64Url(credential.response.clientDataJSON), attestationObject: encodeBase64Url(credential.response.attestationObject), transports: credential.response.getTransports().filter((value): value is z.infer<typeof transport> => transport.safeParse(value).success) } };
}
export function assertionCredential(credential: Credential | null) {
  if (!(credential instanceof PublicKeyCredential) || !(credential.response instanceof AuthenticatorAssertionResponse)) throw new TypeError('Unexpected credential result');
  return { ...common(credential), response: { clientDataJSON: encodeBase64Url(credential.response.clientDataJSON), authenticatorData: encodeBase64Url(credential.response.authenticatorData), signature: encodeBase64Url(credential.response.signature), userHandle: credential.response.userHandle === null ? null : encodeBase64Url(credential.response.userHandle) } };
}
