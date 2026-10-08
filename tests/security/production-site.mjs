// Production build, fixed local upstream, and the same CSP used by the reviewed Caddy edge.
import { createServer, request as upstreamRequest } from 'node:http';
import { readFile } from 'node:fs/promises';
import { extname, resolve, sep } from 'node:path';
const root = resolve(import.meta.dirname, '../..');
const dist = resolve(root, 'apps/identity-web/dist');
const csp = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; frame-ancestors 'none'; object-src 'none'; base-uri 'none'; form-action 'self'";
const server = createServer(async (request, response) => {
  const url = new URL(request.url ?? '/', 'http://localhost:5290');
  const protocol = /^\/oauth\/(authorize|token|jwks|userinfo|introspect|revoke|logout)(?:\/|$)/u.test(url.pathname);
  if (url.pathname.startsWith('/api/') || url.pathname.startsWith('/health/') || protocol || url.pathname === '/.well-known/openid-configuration') {
    const upstream = upstreamRequest(new URL(url.pathname + url.search, 'http://127.0.0.1:5291'), { method: request.method, headers: { ...request.headers, host: 'localhost:5290' }, timeout: 5000 }, (received) => { response.writeHead(received.statusCode ?? 503, received.headers); received.pipe(response); });
    upstream.on('error', () => { if (!response.headersSent) response.writeHead(503, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' }); response.end(); }); upstream.on('timeout', () => upstream.destroy()); request.pipe(upstream); return;
  }
  const file = resolve(dist, `.${url.pathname}`);
  if (file !== dist && !file.startsWith(dist + sep)) { response.writeHead(400); response.end(); return; }
  try {
    const path = extname(url.pathname) ? file : resolve(dist, 'index.html');
    const bytes = await readFile(path);
    const mime = { '.html': 'text/html; charset=utf-8', '.js': 'application/javascript', '.css': 'text/css', '.svg': 'image/svg+xml' }[extname(path)] ?? 'application/octet-stream';
    response.writeHead(200, { 'Content-Type': mime, 'Content-Security-Policy': csp, 'Cache-Control': url.pathname.startsWith('/assets/') ? 'public, max-age=31536000, immutable' : 'no-store', 'Referrer-Policy': 'no-referrer', 'X-Content-Type-Options': 'nosniff' }); response.end(bytes);
  } catch { response.writeHead(404, { 'Content-Type': 'text/plain', 'X-Content-Type-Options': 'nosniff' }); response.end('Not found'); }
});
server.listen(5290, '127.0.0.1', () => { console.log('PRODUCTION_SITE_READY'); });
process.on('SIGTERM', () => server.close());
