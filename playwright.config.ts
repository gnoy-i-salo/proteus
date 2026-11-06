import { defineConfig } from '@playwright/test';
export default defineConfig({
  fullyParallel: true,
  forbidOnly: Boolean(process.env['CI']),
  retries: process.env['CI'] ? 2 : 0,
  reporter: [['list'], ['html', { open: 'never' }]],
  use: {
    browserName: 'chromium',
    ...(process.env['PLAYWRIGHT_CHANNEL'] ? { channel: process.env['PLAYWRIGHT_CHANNEL'] } : {}),
    viewport: { width: 1200, height: 1100 },
    trace: 'retain-on-failure',
  },
  projects: [
    {
      name: 'desktop',
      testDir: './apps/desktop/e2e',
      use: { baseURL: 'http://127.0.0.1:1420' },
    },
    {
      name: 'auth',
      testDir: './tests/e2e',
      testMatch: 'auth.spec.ts',
      use: { baseURL: 'http://127.0.0.1:1421/proteus/auth/' },
    },
  ],
  webServer: [
    {
      command: 'pnpm dev',
      url: 'http://127.0.0.1:1420',
      reuseExistingServer: !process.env['CI'],
    },
    {
      command: 'pnpm --filter @proteus-int/auth build && pnpm --filter @proteus-int/auth preview',
      url: 'http://127.0.0.1:1421/proteus/auth/',
      reuseExistingServer: false,
    },
  ],
});
