import { test, expect } from "@playwright/test";

test.use({ baseURL: "http://localhost:8999" });

test("screenshot of movie playing on live server", async ({ page }) => {
  test.setTimeout(30000);

  await page.goto("/login");
  await page.fill('input[placeholder*="ser"]', "admin");
  await page.fill('input[placeholder*="ass"]', "password");
  await page.click('button[type="submit"]');
  await page.waitForURL("**/", { timeout: 10000 });

  await page.goto("/player/demo");
  await page.waitForTimeout(3000);

  const playBtn = page.locator(".vjs-big-play-button");
  if (await playBtn.isVisible()) {
    await playBtn.click();
    await page.waitForTimeout(5000);
  }

  await page.screenshot({ path: "test-results/demo-playing-live.png" });

  const playing = await page.evaluate(() => {
    const v = document.querySelector("video");
    return v ? { paused: v.paused, currentTime: v.currentTime, duration: v.duration, readyState: v.readyState } : null;
  });
  console.log("Video state:", JSON.stringify(playing));
});
