import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [react()],
  server: {
    host: process.env.HOST ?? '127.0.0.1',
    port: 5175,
    strictPort: true,
    proxy: { '/bff': process.env.BFF_API_PROXY ?? 'http://127.0.0.1:8083' },
  },
});
