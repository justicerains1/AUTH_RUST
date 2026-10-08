import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [react()],
  server: {
    host: process.env.HOST ?? '127.0.0.1',
    port: 5173,
    strictPort: true,
    proxy: {
      '/api': process.env.IDENTITY_API_PROXY ?? 'http://127.0.0.1:8080',
      '/health': process.env.IDENTITY_API_PROXY ?? 'http://127.0.0.1:8080',
      '^/\\.well-known/openid-configuration(?:\\?|$)':
        process.env.IDENTITY_API_PROXY ?? 'http://127.0.0.1:8080',
      '^/oauth/(authorize|token|jwks|userinfo|introspect|revoke|logout)(?:[/?]|$)':
        process.env.IDENTITY_API_PROXY ?? 'http://127.0.0.1:8080',
    },
  },
});
