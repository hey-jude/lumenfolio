import { defineConfig } from '@playwright/test'

const e2ePort = Number(process.env.LUMENFOLIO_E2E_PORT || 1437)
const baseURL = `http://127.0.0.1:${e2ePort}`

export default defineConfig({
  testDir: './tests/e2e',
  fullyParallel: true,
  reporter: [['list']],
  webServer: {
    command: `VITE_E2E=1 LUMENFOLIO_DEV_PORT=${e2ePort} LUMENFOLIO_STRICT_PORT=1 npm run dev`,
    url: baseURL,
    reuseExistingServer: false,
    timeout: 120_000,
  },
  use: {
    baseURL,
    browserName: 'chromium',
    channel: 'chrome',
    trace: 'retain-on-failure',
  },
})
