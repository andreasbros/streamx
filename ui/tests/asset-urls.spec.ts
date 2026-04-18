import { test, expect } from "@playwright/test";

const SITE = "https://streamx.cbdemo.net";

test.describe("All assets served via /assets/{hash}/", () => {
  test("index.html rewrites all static paths to versioned URLs", async ({ page }) => {
    const response = await page.goto(SITE, { waitUntil: "domcontentloaded" });
    expect(response?.status()).toBe(200);

    const html = await page.content();

    // Extract the build hash from the first versioned asset URL
    const hashMatch = html.match(/\/assets\/([a-f0-9]+)\//);
    expect(hashMatch, "Should find build hash in asset URLs").toBeTruthy();
    const buildHash = hashMatch![1];
    console.log(`Build hash: ${buildHash}`);

    // All icon refs in <head> must go through /assets/{hash}/
    const iconHrefs = await page.$$eval(
      'link[rel="icon"], link[rel="apple-touch-icon"]',
      (links) => links.map((l) => l.getAttribute("href"))
    );
    console.log("Icon hrefs:", iconHrefs);
    for (const href of iconHrefs) {
      expect(href, `Icon ${href} should be versioned`).toMatch(
        /^\/assets\/[a-f0-9]+\//
      );
    }

    // JS and CSS bundles must go through /assets/{hash}/
    const scriptSrcs = await page.$$eval("script[src]", (scripts) =>
      scripts.map((s) => s.getAttribute("src"))
    );
    for (const src of scriptSrcs) {
      if (src && src.includes("/assets/")) {
        expect(src, `Script ${src} should be versioned`).toContain(
          `/assets/${buildHash}/`
        );
      }
    }

    const stylesheetHrefs = await page.$$eval(
      'link[rel="stylesheet"]',
      (links) => links.map((l) => l.getAttribute("href"))
    );
    for (const href of stylesheetHrefs) {
      if (href && href.includes("/assets/")) {
        expect(href, `Stylesheet ${href} should be versioned`).toContain(
          `/assets/${buildHash}/`
        );
      }
    }
  });

  test("versioned icon URLs return 200 with immutable cache", async ({
    request,
  }) => {
    // First get the build hash from index.html
    const indexRes = await request.get(SITE);
    const html = await indexRes.text();
    const hashMatch = html.match(/\/assets\/([a-f0-9]+)\//);
    expect(hashMatch).toBeTruthy();
    const buildHash = hashMatch![1];

    const iconFiles = [
      "icons/favicon.svg",
      "icons/icon-32.png",
      "icons/icon-16.png",
      "icons/apple-touch-icon.png",
      "icons/logo.svg",
      "icons/favicon.ico",
    ];

    for (const icon of iconFiles) {
      const url = `${SITE}/assets/${buildHash}/${icon}`;
      const res = await request.get(url);
      console.log(`${icon}: ${res.status()} cache=${res.headers()["cache-control"]}`);
      expect(res.status(), `${url} should return 200`).toBe(200);
      expect(
        res.headers()["cache-control"],
        `${url} should have immutable cache`
      ).toContain("immutable");
    }
  });

  test("versioned JS/CSS bundles return 200 with immutable cache", async ({
    request,
  }) => {
    const indexRes = await request.get(SITE);
    const html = await indexRes.text();

    // Extract all /assets/{hash}/... URLs from the HTML
    const assetUrls = [
      ...html.matchAll(/["']\/assets\/[a-f0-9]+\/([^"']+)["']/g),
    ].map((m) => m[0].replace(/["']/g, ""));

    console.log("Asset URLs found:", assetUrls);
    expect(assetUrls.length).toBeGreaterThan(0);

    for (const path of assetUrls) {
      if (path.endsWith(".map")) continue;
      const url = `${SITE}${path}`;
      const res = await request.get(url);
      console.log(`${path}: ${res.status()} cache=${res.headers()["cache-control"]}`);
      expect(res.status(), `${url} should return 200`).toBe(200);
      expect(
        res.headers()["cache-control"],
        `${url} should have immutable cache`
      ).toContain("immutable");
    }
  });

  test("Vite content-hashed assets (in JS) return 200", async ({
    request,
  }) => {
    // These are referenced in JS bundles without build hash prefix
    // but with Vite content hash in filename
    const indexRes = await request.get(SITE);
    const html = await indexRes.text();

    // Get the JS bundle URL
    const jsMatch = html.match(/\/assets\/[a-f0-9]+\/(index-[^"']+\.js)/);
    expect(jsMatch).toBeTruthy();
    const hashMatch = html.match(/\/assets\/([a-f0-9]+)\//);
    const jsBundleUrl = `${SITE}/assets/${hashMatch![1]}/${jsMatch![1]}`;

    const jsRes = await request.get(jsBundleUrl);
    const jsContent = await jsRes.text();

    // Find Vite content-hashed asset references in the JS bundle
    const viteAssets = [
      ...jsContent.matchAll(/["']\/assets\/([\w.-]+\.(jpg|png|svg|webp))["']/g),
    ].map((m) => m[1]);

    console.log("Vite content-hashed assets in JS:", viteAssets);

    for (const asset of viteAssets) {
      const url = `${SITE}/assets/${asset}`;
      const res = await request.get(url);
      console.log(`/assets/${asset}: ${res.status()} cache=${res.headers()["cache-control"]}`);
      expect(res.status(), `/assets/${asset} should return 200`).toBe(200);
      expect(
        res.headers()["cache-control"],
        `/assets/${asset} should have immutable cache`
      ).toContain("immutable");
    }
  });

  test("sw.js served via versioned path with Service-Worker-Allowed header", async ({
    request,
  }) => {
    const indexRes = await request.get(SITE);
    const html = await indexRes.text();
    const hashMatch = html.match(/\/assets\/([a-f0-9]+)\//);
    expect(hashMatch).toBeTruthy();
    const buildHash = hashMatch![1];

    // sw.js meta tag should point to versioned path
    const swUrlMatch = html.match(/name="sw-url"\s+content="([^"]+)"/);
    expect(swUrlMatch, "sw-url meta tag should exist").toBeTruthy();
    expect(swUrlMatch![1]).toContain(`/assets/${buildHash}/sw.js`);
    console.log(`SW URL: ${swUrlMatch![1]}`);

    // Versioned sw.js should return 200 with immutable cache + Service-Worker-Allowed
    const swRes = await request.get(`${SITE}/assets/${buildHash}/sw.js`);
    expect(swRes.status()).toBe(200);
    console.log(`sw.js cache: ${swRes.headers()["cache-control"]}`);
    console.log(`sw.js SW-Allowed: ${swRes.headers()["service-worker-allowed"]}`);
    expect(swRes.headers()["cache-control"]).toContain("immutable");
    expect(swRes.headers()["service-worker-allowed"]).toBe("/");
  });

  test("index.html has no-cache headers", async ({ request }) => {
    // Check against localhost since CF tunnel may override cache headers
    const LOCAL = "http://localhost:8999";
    const indexRes = await request.get(LOCAL);
    expect(indexRes.headers()["cache-control"]).toContain("no-cache");
  });

  test("no un-versioned icon/asset paths in served HTML", async ({
    request,
  }) => {
    const indexRes = await request.get(SITE);
    const html = await indexRes.text();

    // Should NOT find any direct /icons/ references (all should be /assets/{hash}/icons/)
    const directIconRefs = html.match(/href="\/icons\//g);
    expect(
      directIconRefs,
      "No direct /icons/ paths should exist in served HTML"
    ).toBeNull();

    // Should NOT find direct /default-poster.jpg or /sw.js
    expect(html).not.toContain('"/default-poster.jpg"');
    expect(html).not.toContain('"/sw.js"');
  });
});
