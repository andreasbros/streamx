import { test, expect } from "@playwright/test";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const STREAM_ID = process.env.STREAMX_STREAM_ID || "";
const QUALITY = process.env.STREAMX_QUALITY || "source";
const TOKEN = process.env.STREAMX_TOKEN || "";
const SCREENSHOT_PATH =
  process.env.STREAMX_SCREENSHOT_PATH ||
  "test-results/transcode-playback.png";

test.describe("HLS Transcode Playback", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/login");
    await page.fill('input[placeholder="Username"]', "admin");
    await page.fill('input[type="password"]', "password");
    await page.click('button[type="submit"]');
    await page.waitForURL(/\/(browse|$)/, { timeout: 15000 });
  });

  test("waits for transcode and plays HLS stream", async ({ page }) => {
    test.setTimeout(60000);
    if (!STREAM_ID) {
      test.skip(true, "No STREAMX_STREAM_ID");
      return;
    }

    // Forward browser console to test output for diagnostics
    page.on("console", (msg) =>
      console.log(`BROWSER[${msg.type()}]: ${msg.text()}`),
    );
    page.on("pageerror", (err) => console.log(`PAGE_ERROR: ${err.message}`));

    // Trigger transcode via API to start segment generation
    const playlistUrl = `/api/stream/${STREAM_ID}/playlist.m3u8?quality=${QUALITY}&token=${encodeURIComponent(TOKEN)}`;
    const triggerResp = await page.request.get(playlistUrl);
    expect(triggerResp.ok()).toBeTruthy();

    // Poll until segments are ready
    let playlistContent = "";
    let hasSegments = false;
    for (let i = 0; i < 15; i++) {
      await page.waitForTimeout(2000);
      const resp = await page.request.get(playlistUrl);
      const body = await resp.text();
      if (body.includes("EXTINF:")) {
        hasSegments = true;
        playlistContent = body;
        console.log(`PLAYLIST_READY after ${(i + 1) * 2}s`);
        break;
      }
    }
    expect(hasSegments).toBeTruthy();
    console.log(`PLAYLIST_CONTENT:\n${playlistContent}`);

    // Intercept a test page URL to serve a minimal HTML page (same origin)
    await page.route("**/test-hls-player", async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "text/html",
        body: `<!DOCTYPE html>
<html><head><meta charset="utf-8"></head>
<body style="margin:0;background:#000">
  <video id="v" controls muted playsinline style="width:100%;height:100vh"></video>
</body></html>`,
      });
    });

    await page.goto("/test-hls-player");

    // Load hls.js via addScriptTag (more reliable than inline script)
    const hlsPath = resolve(
      __dirname,
      "../node_modules/hls.js/dist/hls.min.js",
    );
    await page.addScriptTag({ path: hlsPath });

    // Verify hls.js loaded
    const hlsAvailable = await page.evaluate(
      () => typeof (window as Record<string, unknown>).Hls === "function",
    );
    console.log(`HLS_JS_LOADED: ${hlsAvailable}`);
    expect(hlsAvailable).toBeTruthy();

    // Start HLS playback
    console.log(`HLS_URL: ${playlistUrl}`);
    await page.evaluate((url: string) => {
      const video = document.getElementById("v") as HTMLVideoElement;
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const Hls = (window as any).Hls;

      const hls = new Hls({ debug: false, enableWorker: true });
      hls.loadSource(url);
      hls.attachMedia(video);

      hls.on(Hls.Events.MANIFEST_PARSED, () => {
        console.log("HLS_MANIFEST_PARSED");
        video.play().catch((e: Error) =>
          console.error("PLAY_ERROR:", e.message),
        );
      });

      hls.on(Hls.Events.FRAG_LOADED, () => {
        console.log(
          `HLS_FRAG_LOADED: currentTime=${video.currentTime.toFixed(2)}`,
        );
      });

      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      hls.on(Hls.Events.ERROR, (_evt: string, data: any) => {
        console.error(
          `HLS_ERROR: type=${data.type} details=${data.details} fatal=${data.fatal} url=${data.url || ""} response=${data.response?.code || ""}`,
        );
        if (data.fatal) {
          if (data.type === Hls.ErrorTypes.NETWORK_ERROR) {
            console.log("HLS_FATAL_NETWORK: attempting recovery");
            hls.startLoad();
          } else if (data.type === Hls.ErrorTypes.MEDIA_ERROR) {
            console.log("HLS_FATAL_MEDIA: attempting recovery");
            hls.recoverMediaError();
          }
        }
      });

      hls.on(Hls.Events.LEVEL_LOADED, () => {
        console.log("HLS_LEVEL_LOADED");
      });
    }, playlistUrl);

    // Wait for playback to start
    await page.waitForFunction(
      () => {
        const v = document.getElementById("v") as HTMLVideoElement;
        return v && v.currentTime > 0.5 && !v.paused;
      },
      { timeout: 30000 },
    );

    const t0 = await page.evaluate(
      () =>
        (document.getElementById("v") as HTMLVideoElement)?.currentTime ?? 0,
    );
    console.log(`PLAYBACK_STARTED:${t0}`);

    // Wait 3 seconds to prove continuous playback
    await page.waitForTimeout(3000);

    const t1 = await page.evaluate(
      () =>
        (document.getElementById("v") as HTMLVideoElement)?.currentTime ?? 0,
    );
    const delta = t1 - t0;
    console.log(`PLAYBACK_ADVANCED:${t1} delta=${delta}`);
    expect(delta).toBeGreaterThanOrEqual(2);

    // Take screenshot of video
    const video = page.locator("#v");
    await video.screenshot({ path: SCREENSHOT_PATH });

    const state = await page.evaluate(() => {
      const v = document.getElementById("v") as HTMLVideoElement;
      return {
        currentTime: v?.currentTime ?? 0,
        paused: v?.paused ?? true,
        videoWidth: v?.videoWidth ?? 0,
        videoHeight: v?.videoHeight ?? 0,
      };
    });
    console.log(`PLAYBACK_STATE:${JSON.stringify(state)}`);

    expect(state.paused).toBe(false);
    expect(state.videoWidth).toBeGreaterThan(0);
  });
});
