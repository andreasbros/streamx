import { test, expect } from "@playwright/test";

test.use({ baseURL: "http://localhost:8999" });

test("screenshot of movie playing", async ({ page }) => {
  test.setTimeout(60000);

  page.on("console", (msg) => {
    if (msg.type() === "error") console.log("BROWSER ERROR:", msg.text());
  });

  await page.goto("/login");
  await page.fill('input[placeholder*="ser"]', "admin");
  await page.fill('input[placeholder*="ass"]', "password");
  await page.click('button[type="submit"]');
  await page.waitForURL("**/", { timeout: 10000 });

  await page.fill('input[placeholder*="Search"]', "night of the living dead 1968");
  await page.waitForTimeout(2000);

  const firstResult = page.locator('[class*="Card"]').first();
  await firstResult.click();
  await page.waitForURL("**/player/**", { timeout: 10000 });

  await page.waitForTimeout(5000);
  await page.screenshot({ path: "test-results/movie-waiting.png" });

  for (let i = 0; i < 30; i++) {
    const hasVideo = await page.locator(".video-js").count();
    if (hasVideo > 0) {
      await page.waitForTimeout(2000);
      const playBtn = page.locator(".vjs-big-play-button");
      if (await playBtn.isVisible()) {
        await playBtn.click();
        await page.waitForTimeout(3000);
      }
      await page.screenshot({ path: "test-results/movie-playing.png" });
      console.log("VIDEO PLAYER VISIBLE - screenshot taken");
      return;
    }
    await page.waitForTimeout(2000);
    console.log(`Waiting for player... attempt ${i + 1}`);
  }

  await page.screenshot({ path: "test-results/movie-final.png" });
  const state = await page.evaluate(() => ({
    videoCount: document.querySelectorAll("video").length,
    vjsCount: document.querySelectorAll(".video-js").length,
    text: document.body.innerText.substring(0, 300),
  }));
  console.log("Final state:", JSON.stringify(state));
  expect(state.videoCount).toBeGreaterThan(0);
});
