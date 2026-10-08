import { defineConfig } from '@playwright/test';
import { resolve } from 'node:path';

export default defineConfig({
  testDir: '.', testMatch: 'product-accessibility.spec.ts', workers: 1, fullyParallel: false,
  timeout: 60_000, reporter: [['dot']], outputDir: '../../.local/product-accessibility-results',
  projects: [{ name: 'chromium', use: { browserName: 'chromium' } }, { name: 'firefox', use: { browserName: 'firefox' } }],
  use: { baseURL: 'http://localhost:5320', viewport: { width: 390, height: 844 }, reducedMotion: 'reduce', trace: 'off', screenshot: 'off', video: 'off' },
  webServer: {
    command: `"${process.execPath}" "${resolve(import.meta.dirname, '../../node_modules/vite/bin/vite.js')}" --host 127.0.0.1 --port 5320 --strictPort`,
    cwd: resolve('apps/identity-web'), url: 'http://127.0.0.1:5320', reuseExistingServer: false, timeout: 30_000,
    env: { IDENTITY_API_PROXY: 'http://127.0.0.1:5321' },
  },
});
