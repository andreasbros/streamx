import { test, expect } from "@playwright/test";

// These tests verify actual video playback in the browser.
// They use the demo HLS stream (external, always available)
// to prove video.js loads, plays, and advances currentTime.

const DEMO_STREAM_URL = "/player/demo";

test.describe("HLS Playback", () => {
  test.beforeEach(async ({ page }) => {
    // Login first
    await page.goto("/login");
    await page.fill('input[placeholder="Username"]', "admin");
    await page.fill('input[type="password"]', "password");
    await page.click('button[type="submit"]');
    await page.waitForURL(/\/(browse|$)/, { timeout: 10000 });
  });

  test("demo video starts playing within 30 seconds", async ({ page }) => {
    test.setTimeout(60000);

    await page.goto(DEMO_STREAM_URL);

    // Wait for video element to appear
    const video = page.locator("video");
    await expect(video).toBeVisible({ timeout: 15000 });

    // Click play overlay if visible
    const overlay = page.locator('[style*="cursor: pointer"]').first();
    if (await overlay.isVisible({ timeout: 2000 }).catch(() => false)) {
      await overlay.click();
    }

    // Wait for video to start playing (currentTime > 0.5)
    await page.waitForFunction(
      () => {
        const v = document.querySelector("video");
        return v && v.currentTime > 0.5 && !v.paused;
      },
      { timeout: 30000 }
    );

    // Verify playing state
    const state = await page.evaluate(() => {
      const v = document.querySelector("video");
      return {
        currentTime: v?.currentTime ?? 0,
        paused: v?.paused ?? true,
        readyState: v?.readyState ?? 0,
        videoWidth: v?.videoWidth ?? 0,
        videoHeight: v?.videoHeight ?? 0,
      };
    });

    expect(state.currentTime).toBeGreaterThan(0);
    expect(state.paused).toBe(false);
    expect(state.readyState).toBeGreaterThanOrEqual(3);
  });

  test("video currentTime advances over 5 seconds", async ({ page }) => {
    test.setTimeout(60000);

    await page.goto(DEMO_STREAM_URL);
    const video = page.locator("video");
    await expect(video).toBeVisible({ timeout: 15000 });

    // Click play
    const overlay = page.locator('[style*="cursor: pointer"]').first();
    if (await overlay.isVisible({ timeout: 2000 }).catch(() => false)) {
      await overlay.click();
    }

    // Wait for playback to start
    await page.waitForFunction(
      () => {
        const v = document.querySelector("video");
        return v && v.currentTime > 0.5 && !v.paused;
      },
      { timeout: 30000 }
    );

    // Record time at T=0
    const t0 = await page.evaluate(() => document.querySelector("video")?.currentTime ?? 0);

    // Wait 5 seconds
    await page.waitForTimeout(5000);

    // Record time at T=5
    const t1 = await page.evaluate(() => document.querySelector("video")?.currentTime ?? 0);

    // currentTime should have advanced by at least 4 seconds
    const delta = t1 - t0;
    expect(delta).toBeGreaterThanOrEqual(4);
  });

  test("screenshot proves video frames are rendering", async ({ page }) => {
    test.setTimeout(60000);

    await page.goto(DEMO_STREAM_URL);
    const video = page.locator("video");
    await expect(video).toBeVisible({ timeout: 15000 });

    // Click play
    const overlay = page.locator('[style*="cursor: pointer"]').first();
    if (await overlay.isVisible({ timeout: 2000 }).catch(() => false)) {
      await overlay.click();
    }

    // Wait for actual playback
    await page.waitForFunction(
      () => {
        const v = document.querySelector("video");
        return v && v.currentTime > 1 && !v.paused && v.videoWidth > 0;
      },
      { timeout: 30000 }
    );

    // Take screenshot A
    const screenshotA = await page.screenshot({
      clip: { x: 50, y: 100, width: 400, height: 250 },
    });

    // Wait 2 seconds for frames to change
    await page.waitForTimeout(2000);

    // Take screenshot B
    const screenshotB = await page.screenshot({
      clip: { x: 50, y: 100, width: 400, height: 250 },
    });

    // Screenshots should differ (video frames are changing)
    const buffersEqual = Buffer.compare(screenshotA, screenshotB) === 0;
    expect(buffersEqual).toBe(false);
  });

  test("player page loads without errors", async ({ page }) => {
    const errors: string[] = [];
    page.on("pageerror", (err) => errors.push(err.message));

    await page.goto(DEMO_STREAM_URL);
    await page.waitForTimeout(3000);

    // Filter out known non-critical errors
    const critical = errors.filter(
      (e) => !e.includes("ResizeObserver") && !e.includes("NotSupportedError")
    );
    expect(critical).toHaveLength(0);
  });
});
