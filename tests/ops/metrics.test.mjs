import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmod, mkdtemp, readFile, readdir, rm, stat, symlink, utimes, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { createServer } from 'node:tls';
import test from 'node:test';
import { backupCompletion, certificateExpiry, collectMetrics, configuration, publishMetrics, verifiedBackupReceipt } from '../../infra/ops/collect-metrics.mjs';

const root = resolve(import.meta.dirname, '../..');
async function fixture(t) { const directory = await mkdtemp(join(tmpdir(), 'identity-ops-metrics-')); t.after(() => rm(directory, { recursive: true, force: true })); return directory; }
async function completed(directory, completedAt = Math.floor(Date.now() / 1000)) {
  const name = '20261008T150000Z-123.tar.age'; const object = Buffer.from('age-encryption.org/v1\nsynthetic file boundary payload\n'); const digest = createHash('sha256').update(object).digest('hex'); const receipt = { version: 1, completed_at: completedAt, backup_name: name, ciphertext_bytes: object.length, ciphertext_sha256: digest };
  await writeFile(join(directory, name), object, { mode: 0o600 }); await writeFile(join(directory, `${name}.sha256`), `${digest}  ${name}\n`, { mode: 0o600 }); await writeFile(join(directory, 'last-success.json'), JSON.stringify(receipt), { mode: 0o600 }); return { name, object, receipt };
}
async function tlsFixture(directory, t, expired = false) {
  const key = join(directory, 'server.key'); const cert = join(directory, 'server.pem');
  if (expired) {
    const request = join(directory, 'server.csr'); const caKey = join(directory, 'ca.key'); const caCert = join(directory, 'ca.pem'); const caConfig = join(directory, 'ca.cnf');
    execFileSync('openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '30', '-subj', '/CN=Local Test CA', '-keyout', caKey, '-out', caCert], { stdio: ['ignore', 'ignore', 'pipe'] });
    await writeFile(join(directory, 'index.txt'), ''); await writeFile(join(directory, 'serial'), '01\n');
    await writeFile(caConfig, `[ca]\ndefault_ca=local\n[local]\ndatabase=${join(directory, 'index.txt')}\nserial=${join(directory, 'serial')}\nnew_certs_dir=${directory}\nprivate_key=${caKey}\ncertificate=${caCert}\ndefault_md=sha256\npolicy=policy\nx509_extensions=server\n[policy]\ncommonName=supplied\n[server]\nsubjectAltName=DNS:localhost\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n`);
    execFileSync('openssl', ['req', '-new', '-newkey', 'rsa:2048', '-nodes', '-subj', '/CN=localhost', '-keyout', key, '-out', request], { stdio: ['ignore', 'ignore', 'pipe'] });
    execFileSync('openssl', ['ca', '-batch', '-notext', '-config', caConfig, '-in', request, '-startdate', '20200101000000Z', '-enddate', '20200102000000Z', '-out', cert], { stdio: ['ignore', 'ignore', 'pipe'] });
    await chmod(caKey, 0o600);
  } else execFileSync('openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '30', '-subj', '/CN=localhost', '-addext', 'subjectAltName=DNS:localhost', '-keyout', key, '-out', cert], { stdio: ['ignore', 'ignore', 'pipe'] });
  await chmod(key, 0o600);
  const server = createServer({ key: await readFile(key), cert: await readFile(cert) }, (socket) => { socket.end(); }); server.on('tlsClientError', () => {});
  await new Promise((done, fail) => { server.once('error', fail); server.listen(0, '127.0.0.1', done); }); t.after(() => new Promise((done) => server.close(done))); const address = server.address(); assert.ok(address && typeof address !== 'string');
  return { issuer: new URL(`https://localhost:${address.port}`), caFile: expired ? join(directory, 'ca.pem') : cert };
}

test('backup freshness requires the completed object and ignores refreshed mtimes', async (t) => {
  const directory = await fixture(t); const old = Math.floor(Date.now() / 1000) - 90_000; const material = await completed(directory, old);
  assert.equal(await backupCompletion(directory, Math.floor(Date.now() / 1000)), old);
  const verified = await verifiedBackupReceipt(directory, Math.floor(Date.now() / 1000)); assert.deepEqual(verified, material.receipt); assert.ok(Object.isFrozen(verified)); assert.throws(() => { verified.completed_at = Math.floor(Date.now() / 1000); }, TypeError);
  await utimes(join(directory, material.name), new Date(), new Date()); await utimes(join(directory, 'last-success.json'), new Date(), new Date()); assert.equal(await backupCompletion(directory, Math.floor(Date.now() / 1000)), old);
  await t.test('future, path injection and inconsistent metadata are rejected', async () => {
    for (const change of [{ completed_at: Math.floor(Date.now() / 1000) + 120 }, { backup_name: '../elsewhere.tar.age' }, { ciphertext_bytes: material.object.length + 1 }, { ciphertext_sha256: '0'.repeat(64) }, { completed_at: 0 }, { version: 2 }, { unexpected: 'extra' }]) {
      await writeFile(join(directory, 'last-success.json'), JSON.stringify({ ...material.receipt, ...change })); await assert.rejects(backupCompletion(directory, Math.floor(Date.now() / 1000)));
    }
    await writeFile(join(directory, 'last-success.json'), JSON.stringify(material.receipt));
  });
  await t.test('corruption, missing manifest and symlink cannot report a fresh backup', async () => {
    const changed = Buffer.from(material.object); changed[changed.length - 1] ^= 1; await writeFile(join(directory, material.name), changed); await assert.rejects(backupCompletion(directory, Math.floor(Date.now() / 1000))); await writeFile(join(directory, material.name), material.object);
    const manifest = join(directory, `${material.name}.sha256`); await rm(manifest); await assert.rejects(backupCompletion(directory, Math.floor(Date.now() / 1000))); await writeFile(manifest, `${material.receipt.ciphertext_sha256}  ${material.name}\n`, { mode: 0o600 });
    await rm(join(directory, 'last-success.json')); await writeFile(join(directory, 'elsewhere.json'), JSON.stringify(material.receipt), { mode: 0o600 }); await symlink(join(directory, 'elsewhere.json'), join(directory, 'last-success.json')); await assert.rejects(backupCompletion(directory, Math.floor(Date.now() / 1000)));
  });
});

test('actual local TLS validates the trusted issuer and rejects untrusted or wrong-host certificates', async (t) => {
  const directory = await fixture(t); const tls = await tlsFixture(directory, t); const expiry = await certificateExpiry(tls.issuer, tls.caFile); assert.ok(expiry > Date.now() / 1000 + 29 * 86400);
  await assert.rejects(certificateExpiry(tls.issuer)); await assert.rejects(certificateExpiry(new URL(`https://127.0.0.1:${tls.issuer.port}`), tls.caFile));
});

test('an actually expired local certificate fails despite explicit trust and a matching hostname', async (t) => {
  const directory = await fixture(t); const tls = await tlsFixture(directory, t, true); await assert.rejects(certificateExpiry(tls.issuer, tls.caFile), { code: 'CERT_HAS_EXPIRED' });
});

test('collector publishes actual statfs, TLS and completed-backup values with fixed labels', async (t) => {
  const directory = await fixture(t); await completed(directory); const tls = await tlsFixture(directory, t); const expectedDevice = (await stat(directory, { bigint: true })).dev;
  const config = { dataDirectory: directory, dataDevice: expectedDevice, backupDirectory: directory, backupDevice: expectedDevice, ...tls, output: join(directory, 'identity-ops.prom') };
  const result = await collectMetrics(config); assert.equal(result.success, true); assert.deepEqual(result.checks, { disk_data: 1, disk_backup: 1, issuer_tls: 1, base_backup: 1 });
  assert.equal(result.text.includes(directory), false); assert.equal(result.text.includes('localhost'), false); assert.equal(result.text.includes('20261008'), false); assert.equal(/\b(?:NaN|Inf)\b/u.test(result.text), false);
  assert.equal(result.text.match(/^identity_ops_collection_success\{/gmu)?.length, 4); assert.match(result.text, /identity_ops_disk_size_bytes\{volume="data"\} [1-9]\d*/u);
  await publishMetrics(config.output, result.text); assert.equal(await readFile(config.output, 'utf8'), result.text); assert.equal((await stat(config.output)).mode & 0o777, 0o640); assert.equal((await readdir(directory)).some((name) => name.includes('.tmp-')), false);
  const promtool = process.env.PROMTOOL_BINARY ?? join(root, '.local/security-tools/prometheus-3.15.0.linux-amd64/promtool');
  execFileSync(promtool, ['check', 'metrics'], { input: result.text, encoding: 'utf8' });
  await t.test('wrong expected device and incomplete backup publish failures with current attempt times', async () => {
    await rm(join(directory, 'last-success.json')); const failure = await collectMetrics({ ...config, dataDevice: expectedDevice + 1n }); assert.equal(failure.success, false); assert.equal(failure.checks.disk_data, 0); assert.equal(failure.checks.base_backup, 0); assert.match(failure.text, /identity_ops_base_backup_completed_timestamp_seconds 0\n/u); assert.match(failure.text, /identity_ops_collection_timestamp_seconds\{check="base_backup"\} [1-9]\d*/u); await publishMetrics(config.output, failure.text); assert.equal(await readFile(config.output, 'utf8'), failure.text);
  });
  await t.test('unsafe output destinations cannot replace the last metrics file', async () => { await chmod(directory, 0o777); await assert.rejects(publishMetrics(config.output, 'changed\n')); await chmod(directory, 0o700); await rm(config.output); await symlink(join(directory, 'server.pem'), config.output); await assert.rejects(publishMetrics(config.output, 'changed\n')); });
});

test('CLI does not print secret configuration or advertise a failed publication as success', async (t) => {
  const directory = await fixture(t); const secret = 'secret-configuration-never-print';
  const result = await new Promise((done) => { const child = spawn(process.execPath, [join(root, 'infra/ops/collect-metrics.mjs')], { env: { ...process.env, ISSUER: `https://${secret}:password@example.test`, OPS_METRICS_FILE: join(directory, 'identity.prom') }, stdio: ['ignore', 'pipe', 'pipe'] }); let output = ''; child.stdout.on('data', (chunk) => { output += chunk; }); child.stderr.on('data', (chunk) => { output += chunk; }); child.on('close', (code) => done({ code, output })); });
  assert.equal(result.code, 1); assert.equal(result.output.includes(secret), false); assert.equal((await readdir(directory)).length, 0);
  assert.throws(() => configuration({ ISSUER: 'http://example.test' }));
});
