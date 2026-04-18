import { test, expect } from "@playwright/test";

test.use({
  baseURL: "http://localhost:8999",
});

test("demo player via Watch Demo button", async ({ page }) => {
  const logs: string[] = [];
  page.on("console", (msg) => logs.push(`[${msg.type()}] ${msg.text()}`));
  page.on("pageerror", (err) => logs.push(`[ERROR] ${err.message}`));

  await page.goto("/login");
  await page.waitForTimeout(1000);
  await page.fill('input[placeholder*="ser"]', "admin");
  await page.fill('input[placeholder*="ass"]', "password");
  await page.click('button[type="submit"]');
  await page.waitForURL("**/", { timeout: 10000 });

  await page.screenshot({ path: "test-results/01-search-page.png" });

  const demoBtn = page.locator("button", { hasText: "Watch Demo" });
  await expect(demoBtn).toBeVisible({ timeout: 5000 });
  await demoBtn.click();

  await page.waitForTimeout(3000);
  await page.screenshot({ path: "test-results/02-player-page.png" });

  const url = page.url();
  console.log("Current URL:", url);
  console.log("Console logs:", logs.slice(-10));

  const state = await page.evaluate(() => ({
    videoCount: document.querySelectorAll("video").length,
    vjsCount: document.querySelectorAll(".video-js").length,
    textContent: document.body.innerText.substring(0, 300),
  }));

  console.log("State:", JSON.stringify(state));
  expect(url).toContain("/player/demo");
  expect(state.videoCount).toBeGreaterThan(0);
});
