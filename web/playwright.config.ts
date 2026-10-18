import { existsSync } from 'node:fs';
import { defineConfig } from '@playwright/test';

const systemChromium = existsSync('/usr/bin/chromium') ? '/usr/bin/chromium' : undefined;

export default defineConfig({
  testDir: './e2e',
  timeout: 180_000,
  fullyParallel: false,
  workers: 1,
  reporter: 'list',
  use: {
    baseURL: 'http://127.0.0.1:4173',
    launchOptions: systemChromium
      ? { executablePath: systemChromium, args: ['--no-sandbox'] }
      : {},
    trace: 'retain-on-failure'
  },
  webServer: [
    {
      command:
        'cd ../backend && cargo build && rm -f /tmp/diary-e2e.db* && ' +
        'DIARY_BIND=127.0.0.1:8787 DIARY_DB_PATH=/tmp/diary-e2e.db ' +
        'DIARY_JWT_SECRET=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA= ' +
        'DIARY_MASTER_KEY=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA= ' +
        'DIARY_CORS_ORIGINS=http://127.0.0.1:4173 DIARY_SECURE_COOKIE=false ' +
        'DIARY_ALLOW_REGISTRATION=true ./target/debug/diary',
      url: 'http://127.0.0.1:8787/api/v1/healthz',
      reuseExistingServer: false,
      timeout: 120_000
    },
    {
      command: 'npm run build && npm run preview -- --host 127.0.0.1 --port 4173',
      url: 'http://127.0.0.1:4173',
      reuseExistingServer: false,
      timeout: 180_000
    }
  ]
});
