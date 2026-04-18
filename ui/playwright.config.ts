import { defineConfig } from "@playwright/test";

const port = Number(process.env.STREAMX_TEST_PORT) || 9876;
const tmpDir = `/tmp/streamx-test-${Date.now()}`;

export default defineConfig({
  testDir: "./tests",
  timeout: 30000,
  retries: 0,
  workers: 1,
  use: {
    baseURL: `http://localhost:${port}`,
    trace: "on-first-retry",
  },
  projects: [
    {
      name: "chromium",
      use: { browserName: "chromium" },
    },
  ],
  webServer: {
    command: `cd ../backend && cargo run -- --port ${port} --data-dir ${tmpDir} --admin-user admin --admin-password password`,
    port,
    reuseExistingServer: !process.env.CI,
    timeout: 120000,
  },
});
