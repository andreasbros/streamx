import { test } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { dirname } from "node:path";

/**
 * Captures the home page after login for use as a GitHub OG image / README screenshot.
 *
 * Run: npx playwright test tests/screenshot-og.spec.ts --config tests/live.config.ts
 * Output: ../docs/og-preview.png (1280x720, GitHub OG card aspect ratio)
 */

const BASE_URL = process.env.STREAMX_URL ?? "http://localhost:8999";
const USERNAME = process.env.STREAMX_USER ?? "admin";
const PASSWORD = process.env.STREAMX_PASSWORD ?? "password";
const OUTPUT = process.env.STREAMX_SCREENSHOT_PATH ?? "../docs/og-preview.png";

test.use({ baseURL: BASE_URL, viewport: { width: 1280, height: 720 } });

test("capture home page OG screenshot", async ({ page }) => {
  test.setTimeout(60000);

  await page.goto("/login");
  await page.fill('input[placeholder*="ser"]', USERNAME);
  await page.fill('input[placeholder*="ass"]', PASSWORD);
  await page.click('button[type="submit"]');
  await page.waitForURL((url) => !url.pathname.startsWith("/login"), { timeout: 15000 });

  // Let browse sections render
  await page.waitForSelector("text=/Latest|Popular|Top Rated/i", { timeout: 15000 });
  await page.waitForLoadState("networkidle", { timeout: 15000 }).catch(() => {});

  // Force lazy-loaded poster images to load by scrolling them into view briefly,
  // then scroll back to the top so the Latest section is visible in the screenshot.
  await page.evaluate(async () => {
    const imgs = Array.from(document.querySelectorAll("img")).filter((i) => i.loading === "lazy");
    for (const img of imgs) img.loading = "eager";
    // Poke the scroll to trigger the intersection observer
    window.scrollTo(0, 400);
    await new Promise((r) => setTimeout(r, 300));
    window.scrollTo(0, 800);
    await new Promise((r) => setTimeout(r, 300));
    window.scrollTo(0, 0);
    // Wait for images to actually decode
    await Promise.all(
      Array.from(document.querySelectorAll("img")).map((img) =>
        img.complete ? Promise.resolve() : new Promise((r) => img.addEventListener("load", r, { once: true })),
      ),
    );
  });
  await page.waitForTimeout(800);

  await mkdir(dirname(OUTPUT), { recursive: true }).catch(() => {});
  await page.screenshot({ path: OUTPUT, fullPage: false });

  console.log(`Screenshot saved to ${OUTPUT}`);
});
