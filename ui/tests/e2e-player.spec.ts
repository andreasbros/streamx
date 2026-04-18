import { test, expect } from "@playwright/test";

async function loginAsAdmin(request: any) {
  const res = await request.post("/api/auth/login", {
    data: { username: "admin", password: "password" },
  });
  return (await res.json()).token;
}

test.describe("Video Player", () => {
  test("demo player page renders video element", async ({ page, request }) => {
    await loginAsAdmin(request);

    await page.goto("/login");
    await page.fill('input[placeholder*="ser"]', "admin");
    await page.fill('input[placeholder*="ass"]', "password");
    await page.click('button[type="submit"]');
    await page.waitForURL("**/", { timeout: 10000 });

    await page.goto("/player/demo");
    await page.waitForTimeout(3000);

    const errors: string[] = [];
    page.on("pageerror", (err) => errors.push(err.message));

    await page.screenshot({ path: "test-results/player-demo.png" });

    const state = await page.evaluate(() => ({
      videoCount: document.querySelectorAll("video").length,
      vjsCount: document.querySelectorAll(".video-js").length,
      bodyText: document.body.innerText.substring(0, 500),
      consoleErrors: (window as any).__errors || [],
    }));

    console.log("Player state:", JSON.stringify(state));
    console.log("Page errors:", errors);

    expect(state.videoCount).toBeGreaterThan(0);
  });
});
