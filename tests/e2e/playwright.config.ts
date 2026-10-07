import { defineConfig } from '@playwright/test';
import { resolve } from 'node:path';
export default defineConfig({
  testDir: '.', testMatch: 'T05.spec.ts', workers: 1, fullyParallel: false, timeout: 60_000,
  reporter: [['dot']], outputDir: '../../.local/t05-browser-results',
  use: { baseURL: 'http://localhost:5190', viewport: { width: 390, height: 844 }, trace: 'off', screenshot: 'off', video: 'off' },
  webServer: {
    command: `"${process.execPath}" "${resolve(import.meta.dirname, '../../node_modules/vite/bin/vite.js')}" --host 127.0.0.1 --port 5190 --strictPort`,
    cwd: resolve('apps/identity-web'), url: 'http://127.0.0.1:5190', reuseExistingServer: false, timeout: 30_000,
    env: { IDENTITY_API_PROXY: 'http://127.0.0.1:5191' },
  },
});
