import { test, expect } from "@playwright/test";

async function login(page: import("@playwright/test").Page) {
  await page.goto("/login");
  await page.fill('input[placeholder*="ser"]', "admin");
  await page.fill('input[placeholder*="ass"]', "password");
  await page.click('button[type="submit"]');
  await page.waitForURL("**/", { timeout: 10000 });
}

async function getToken(request: any): Promise<string> {
  const res = await request.post("/api/auth/login", {
    data: { username: "admin", password: "password" },
  });
  const body = await res.json();
  return body.token;
}

test.describe("Navigation", () => {
  test("search -> click result -> player -> back returns to search with results", async ({
    page,
  }) => {
    test.setTimeout(60000);

    await login(page);

    // Search for "inc"
    const searchInput = page.locator('input[placeholder*="Search"]');
    await searchInput.fill("inc");
    await page.waitForTimeout(2000);

    // Wait for results to appear
    const resultTitle = page.locator("text=Food, Inc").first();
    await expect(resultTitle).toBeVisible({ timeout: 10000 });
    await page.screenshot({ path: "test-results/nav-01-search-results.png" });

    // Click the title to expand the group card
    await resultTitle.click();
    await page.waitForTimeout(1000);
    await page.screenshot({ path: "test-results/nav-01b-expanded.png" });

    // Click a variant row in the expanded table (rows have border-top style)
    // The variant table rows are inside a bordered container after the separator
    const variantRows = page.locator('[style*="cursor: pointer"][style*="gap"]');
    // Skip the first one (the collapsed card header) and click a variant row
    const count = await variantRows.count();
    console.log(`Found ${count} clickable rows`);
    for (let i = 0; i < count; i++) {
      const row = variantRows.nth(i);
      const text = await row.textContent();
      // Variant rows contain size info like "MB" or "GB"
      if (text && (text.includes("MB") || text.includes("GB"))) {
        console.log(`Clicking variant row ${i}: ${text?.substring(0, 60)}`);
        await row.click();
        break;
      }
    }
    await page.waitForURL("**/player/**", { timeout: 15000 });
    await page.screenshot({ path: "test-results/nav-02-player.png" });

    // Click the Back button
    const backButton = page.getByRole("button", { name: "Back", exact: true });
    await expect(backButton).toBeVisible({ timeout: 5000 });
    await backButton.click();

    // Should be back on search page at "/"
    await page.waitForURL("**/", { timeout: 10000 });

    // The search input should still have "inc" (restored from sessionStorage)
    await expect(searchInput).toHaveValue("inc", { timeout: 5000 });

    // Results should re-appear
    await page.waitForTimeout(2000);
    const resultsAfterBack = page.locator("text=Food, Inc").first();
    await expect(resultsAfterBack).toBeVisible({ timeout: 10000 });
    await page.screenshot({ path: "test-results/nav-03-back-to-search.png" });
  });

  test("history page loads without errors", async ({ page }) => {
    test.setTimeout(30000);

    const errors: string[] = [];
    page.on("pageerror", (err) => errors.push(err.message));

    await login(page);

    await page.goto("/history");
    await page.waitForTimeout(2000);

    await page.screenshot({ path: "test-results/nav-04-history.png" });

    const heading = page.locator("text=Watch History");
    await expect(heading).toBeVisible({ timeout: 5000 });

    expect(errors).toHaveLength(0);

    const emptyState = page.locator("text=Nothing watched yet");
    const historyCard = page.locator('[class*="Card"]').first();
    const eitherVisible =
      (await emptyState.isVisible().catch(() => false)) ||
      (await historyCard.isVisible().catch(() => false));
    expect(eitherVisible).toBe(true);
  });

  test("after starting a stream, it appears in history", async ({
    page,
    request,
  }) => {
    test.setTimeout(60000);

    const token = await getToken(request);
    const headers = { Authorization: `Bearer ${token}` };

    // Search returns grouped results
    const searchRes = await request.post("/api/search", {
      headers,
      data: { query: "inc" },
    });
    expect(searchRes.ok()).toBeTruthy();
    const groups = (await searchRes.json()).results;
    expect(groups.length).toBeGreaterThan(0);

    // Start a stream using the first variant's magnet
    const group = groups[0];
    const variant = group.variants[0];
    const streamRes = await request.post("/api/stream", {
      headers,
      data: { magnet_uri: variant.magnet },
    });
    expect(streamRes.ok()).toBeTruthy();

    // Now login via the browser and check history
    await login(page);
    await page.goto("/history");
    await page.waitForTimeout(2000);

    await page.screenshot({ path: "test-results/nav-05-history-with-entry.png" });

    const emptyState = page.locator("text=Nothing watched yet");
    await expect(emptyState).not.toBeVisible({ timeout: 5000 });

    const historyCard = page.locator('[class*="Card"]').first();
    await expect(historyCard).toBeVisible({ timeout: 5000 });
  });
});
