import { chmod, mkdir, readFile, rm, stat, writeFile } from 'node:fs/promises';
import { randomBytes, createPrivateKey } from 'node:crypto';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { CommandError, runProcess } from './process.mjs';

const LOCAL_FILES = ['signing.pem', 'encryption-keys.json', 'dev.env'];

export async function generateDevSecrets(root = resolve(dirname(fileURLToPath(import.meta.url)), '..')) {
  const local = resolve(root, '.local');
  await mkdir(local, { recursive: true, mode: 0o700 });
  await chmod(local, 0o700);
  for (const file of LOCAL_FILES) {
    try {
      await stat(resolve(local, file));
      throw new CommandError('.local 中已存在本地秘密文件；为保护现有数据库和密钥，请勿自动覆盖。');
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
    }
  }
  const created = [];
  try {
    // Reserve the private key with restrictive permissions before OpenSSL writes it.
    const signingPath = resolve(local, 'signing.pem');
    await writeFile(signingPath, '', { mode: 0o600, flag: 'wx' });
    created.push(signingPath);
    await runProcess('openssl', ['genpkey', '-algorithm', 'RSA', '-pkeyopt', 'rsa_keygen_bits:3072', '-out', signingPath], { quiet: true });
    await chmod(signingPath, 0o600);
    const privateKey = createPrivateKey(await readFile(signingPath));
    if (privateKey.asymmetricKeyType !== 'rsa' || privateKey.asymmetricKeyDetails.modulusLength < 3072) {
      throw new CommandError('生成的本地签名密钥未达到 RSA 3072 位要求。');
    }
    const encryptionPath = resolve(local, 'encryption-keys.json');
    await writeFile(encryptionPath, `${JSON.stringify({ 'local-aead-1': randomBytes(32).toString('base64') })}\n`, { mode: 0o600, flag: 'wx' });
    created.push(encryptionPath);
    const databasePassword = randomBytes(32).toString('hex');
    const values = [
      'APP_ENV=development',
      'BIND=0.0.0.0:8080',
      'ISSUER=http://localhost:5173',
      'RP_ID=localhost',
      `POSTGRES_PASSWORD=${databasePassword}`,
      `DATABASE_URL=postgres://identity:${databasePassword}@postgres:5432/identity_development`,
      'REDIS_URL=redis://redis:6379',
      'SIGNING_KEY_FILE=/run/secrets/signing.pem',
      'SIGNING_KID=local-signing-1',
      'ENCRYPTION_KEYS_FILE=/run/secrets/encryption-keys.json',
      'ACTIVE_ENCRYPTION_KID=local-aead-1',
      'SMTP_HOST=mailpit',
      'SMTP_PORT=1025',
      'SMTP_FROM=no-reply@localhost',
      'SMTP_TLS=disabled',
    ];
    const environmentPath = resolve(local, 'dev.env');
    await writeFile(environmentPath, `${values.join('\n')}\n`, { mode: 0o600, flag: 'wx' });
    created.push(environmentPath);
    for (const path of created) await chmod(path, 0o600);
    console.log('已生成 .local/signing.pem、.local/encryption-keys.json 和 .local/dev.env（不回显秘密）。Windows 用户请将 .local 的访问权限限制为当前账号。');
  } catch (error) {
    for (const path of created) await rm(path, { force: true });
    if (error instanceof CommandError) throw error;
    throw new CommandError('生成本地秘密失败；检查 .local 目录写权限和 OpenSSL 安装。');
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    await generateDevSecrets();
  } catch (error) {
    console.error(error.message);
    process.exitCode = error.exitCode ?? 1;
  }
}
