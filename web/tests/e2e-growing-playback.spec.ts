import { test, expect } from "@playwright/test";

// These env vars are passed from the Rust test harness
const STREAM_ID = process.env.STREAMX_STREAM_ID || "";
const TOKEN = process.env.STREAMX_TOKEN || "";
const QUALITY = process.env.STREAMX_QUALITY || "source";
const SCREENSHOT_PATH = process.env.STREAMX_SCREENSHOT_PATH || "test-results/playback.png";

test.describe("Growing File HLS Playback", () => {
  test.beforeEach(async ({ page }) => {
    // Login
    await page.goto("/login");
    await page.fill('input[placeholder="Username"]', "admin");
    await page.fill('input[type="password"]', "password");
    await page.click('button[type="submit"]');
    await page.waitForURL(/\/(browse|$)/, { timeout: 15000 });
  });

  test("plays growing file HLS stream to target frame", async ({ page }) => {
    test.setTimeout(120000);

    if (!STREAM_ID) {
      test.skip(true, "STREAMX_STREAM_ID not set");
      return;
    }

    // Navigate to the player page with the seeded stream
    await page.goto(`/player/${STREAM_ID}`);

    // Wait for video element to appear
    const video = page.locator("video");
    await expect(video).toBeVisible({ timeout: 30000 });

    // Click play overlay if visible
    const overlay = page.locator('[style*="cursor: pointer"]').first();
    if (await overlay.isVisible({ timeout: 3000 }).catch(() => false)) {
      await overlay.click();
    }

    // Wait for video to start playing
    await page.waitForFunction(
      () => {
        const v = document.querySelector("video");
        return v && v.currentTime > 0.5 && !v.paused;
      },
      { timeout: 60000 }
    );

    console.log("PLAYBACK_STARTED");

    // Wait for video to advance past 8 seconds (enough frames for verification)
    await page.waitForFunction(
      () => {
        const v = document.querySelector("video");
        return v && v.currentTime > 8;
      },
      { timeout: 90000 }
    );

    // Pause the video to capture a clean frame
    await page.evaluate(() => {
      document.querySelector("video")?.pause();
    });
    await page.waitForTimeout(500);

    // Capture the exact currentTime for golden frame extraction
    const finalState = await page.evaluate(() => {
      const v = document.querySelector("video");
      return {
        currentTime: v?.currentTime ?? 0,
        videoWidth: v?.videoWidth ?? 0,
        videoHeight: v?.videoHeight ?? 0,
        paused: v?.paused ?? true,
        readyState: v?.readyState ?? 0,
      };
    });

    console.log(`PLAYBACK_STATE:${JSON.stringify(finalState)}`);

    // Take screenshot of the video element only (no overlays)
    await video.screenshot({ path: SCREENSHOT_PATH });
    console.log(`SCREENSHOT_SAVED:${SCREENSHOT_PATH}`);

    // Assert playback reached target
    expect(finalState.currentTime).toBeGreaterThan(8);
    expect(finalState.videoWidth).toBeGreaterThan(0);
    expect(finalState.videoHeight).toBeGreaterThan(0);
  });
});
