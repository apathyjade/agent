import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: 'list',
  use: {
    // Tauri dev server or production build
    baseURL: 'http://localhost:1420',
    trace: 'on-first-retry',
  },
  // For Tauri E2E testing, run the app with `cargo tauri dev` first
  // Then point tests to the webview URL (default: http://localhost:1420)
  webServer: process.env.CI
    ? {
        command: 'cd ../src-ui && npm run dev',
        url: 'http://localhost:1420',
        reuseExistingServer: !process.env.CI,
      }
    : undefined,
});
