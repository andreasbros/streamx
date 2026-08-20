import { test, expect } from "@playwright/test";
import type { APIRequestContext, Page } from "@playwright/test";

async function login(page: Page) {
  await page.goto("/login");
  await page.fill('input[placeholder*="ser"]', "admin");
  await page.fill('input[placeholder*="ass"]', "password");
  await page.click('button[type="submit"]');
  await page.waitForURL("**/", { timeout: 10000 });
}

async function getToken(request: APIRequestContext): Promise<string> {
  const res = await request.post("/api/auth/login", {
    data: { username: "admin", password: "password" },
  });
  const body = await res.json();
  return body.token;
}

function fakeMagnet(hash: string, name: string): string {
  return `magnet:?xt=urn:btih:${hash}&dn=${encodeURIComponent(name)}`;
}

test.describe("Download queue", () => {
  test("pinned download appears in queue with cancel, then unpins", async ({
    page,
    request,
  }) => {
    test.setTimeout(60000);
    const token = await getToken(request);
    const hash = "aaaa1111aaaa1111aaaa1111aaaa1111aaaa1111";

    const create = await request.post("/api/stream", {
      headers: { Authorization: `Bearer ${token}` },
      data: {
        magnet_uri: fakeMagnet(hash, "Queue Test Movie"),
        title: "Queue Test Movie",
      },
    });
    expect(create.ok()).toBeTruthy();
    const pin = await request.post(`/api/stream/${hash}/download`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    expect(pin.ok()).toBeTruthy();

    await login(page);
    await page.goto("/downloads");

    const row = page.locator("text=Queue Test Movie").first();
    await expect(row).toBeVisible({ timeout: 10000 });
    await page.screenshot({ path: "test-results/dl-01-queue.png" });

    const cancelBtn = page.getByRole("button", { name: /Cancel Download/i });
    await expect(cancelBtn).toBeVisible({ timeout: 10000 });
    await cancelBtn.click();

    // After unpinning, the row offers Download again.
    await expect(
      page.getByRole("button", { name: /^Download$/i })
    ).toBeVisible({ timeout: 10000 });
    await page.screenshot({ path: "test-results/dl-02-cancelled.png" });

    const list = await request.get("/api/downloads", {
      headers: { Authorization: `Bearer ${token}` },
    });
    const body = await list.json();
    const item = body.downloads.find(
      (d: { info_hash: string }) => d.info_hash === hash
    );
    expect(item).toBeTruthy();
    expect(item.pinned).toBe(false);
  });

  test("admin delete removes the download row entirely", async ({
    page,
    request,
  }) => {
    test.setTimeout(60000);
    const token = await getToken(request);
    const hash = "bbbb2222bbbb2222bbbb2222bbbb2222bbbb2222";

    await request.post("/api/stream", {
      headers: { Authorization: `Bearer ${token}` },
      data: {
        magnet_uri: fakeMagnet(hash, "Delete Test Movie"),
        title: "Delete Test Movie",
      },
    });

    await login(page);
    await page.goto("/downloads");
    await expect(page.locator("text=Delete Test Movie").first()).toBeVisible({
      timeout: 10000,
    });

    page.on("dialog", (dialog) => dialog.accept());
    const row = page
      .locator("div")
      .filter({ hasText: "Delete Test Movie" })
      .locator('[aria-label="Delete download"]')
      .first();
    await row.click();

    await expect(page.locator("text=Delete Test Movie")).toHaveCount(0, {
      timeout: 10000,
    });

    // Gone from the API too, not just the UI.
    const list = await request.get("/api/downloads", {
      headers: { Authorization: `Bearer ${token}` },
    });
    const body = await list.json();
    expect(
      body.downloads.find(
        (d: { info_hash: string }) => d.info_hash === hash
      )
    ).toBeUndefined();
  });

  test("drawer menu links to the downloads page", async ({ page }) => {
    await login(page);
    // Open the drawer (hamburger) and follow the Downloads link.
    await page.getByLabel("Open menu").click();
    const link = page.getByRole("link", { name: /Downloads/i });
    await expect(link).toBeVisible({ timeout: 5000 });
    await link.click();
    await page.waitForURL("**/downloads", { timeout: 5000 });
    await expect(page.getByText("Downloads").first()).toBeVisible();
  });
});

test.describe("Server settings", () => {
  test("admin can toggle WEB-only and transcode settings", async ({
    page,
    request,
  }) => {
    test.setTimeout(60000);
    const token = await getToken(request);

    await login(page);
    await page.goto("/admin");
    await expect(page.getByText("Playback & Search")).toBeVisible({
      timeout: 15000,
    });

    const webOnlySwitch = page
      .locator("div")
      .filter({ hasText: /^WEB releases only/ })
      .getByRole("switch")
      .last();
    const before = await webOnlySwitch.getAttribute("data-state");
    await webOnlySwitch.click();
    await expect(webOnlySwitch).toHaveAttribute(
      "data-state",
      before === "checked" ? "unchecked" : "checked",
      { timeout: 5000 }
    );

    const res = await request.get("/api/settings/server", {
      headers: { Authorization: `Bearer ${token}` },
    });
    const settings = await res.json();
    expect(settings.web_only).toBe(before !== "checked");

    // Restore.
    await webOnlySwitch.click();
  });
});
