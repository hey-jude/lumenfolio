import { defineConfig } from '@playwright/test'

const e2ePort = Number(process.env.LUMENFOLIO_E2E_PORT || 1437)
const baseURL = `http://127.0.0.1:${e2ePort}`

export default defineConfig({
  testDir: './tests/e2e',
  fullyParallel: true,
  reporter: [['list']],
  webServer: {
    // Keep the web server command shell-independent. The previous inline
    // POSIX environment assignment fails before Vite starts on Windows.
    command: 'node node_modules/vite/bin/vite.js --host 127.0.0.1',
    env: {
      ...process.env,
      VITE_E2E: '1',
      LUMENFOLIO_DEV_PORT: String(e2ePort),
      LUMENFOLIO_STRICT_PORT: '1',
    },
    url: baseURL,
    reuseExistingServer: false,
    timeout: 120_000,
  },
  use: {
    baseURL,
    browserName: 'chromium',
    channel: process.env.LUMENFOLIO_E2E_BROWSER_CHANNEL || 'chrome',
    trace: 'retain-on-failure',
  },
})
