import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: ".",
  timeout: 180000,
  workers: 1,
  use: {
    baseURL: "http://localhost:8999",
    viewport: { width: 1440, height: 900 },
    screenshot: "on",
  },
  projects: [{ name: "chromium", use: { browserName: "chromium" } }],
  outputDir: "../test-results",
});
