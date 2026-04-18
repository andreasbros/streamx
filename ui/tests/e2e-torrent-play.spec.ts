import { test, expect } from "@playwright/test";

// Uses default playwright.config.ts which starts a fresh backend on port 9876

async function login(page: import("@playwright/test").Page) {
  await page.goto("/login");
  await page.fill('input[placeholder*="ser"]', "admin");
  await page.fill('input[placeholder*="ass"]', "password");
  await page.click('button[type="submit"]');
  await page.waitForURL("**/", { timeout: 10000 });
}

test("search and play a torrent shows player with stream status", async ({
  page,
}) => {
  test.setTimeout(60000);

  const logs: string[] = [];
  page.on("console", (msg) => logs.push(`[${msg.type()}] ${msg.text()}`));

  await login(page);

  // Search for a well-seeded torrent (Food Inc has 22+ seeds)
  await page.fill('input[placeholder*="Search"]', "inc");
  await page.waitForTimeout(2000);
  await page.screenshot({ path: "test-results/torrent-01-search.png" });

  // Click Food Inc result
  const result = page.locator("text=Food, Inc");
  const target = result;
  await expect(target.first()).toBeVisible({ timeout: 10000 });
  await target.first().click();
  await page.waitForURL("**/player/**", { timeout: 15000 });

  console.log("Player URL:", page.url());
  await page.screenshot({ path: "test-results/torrent-02-player-initial.png" });

  // Verify the player page is showing (not stuck on login or search)
  const playerContent = page.locator("text=Stream Status");
  const waitingText = page.locator("text=Waiting for file");
  const connectingText = page.locator("text=Connecting");
  const videoBox = page.locator("video, .video-js");

  // Wait for either the stream status card or the video to appear
  let streamStatusVisible = false;
  let videoVisible = false;

  for (let i = 0; i < 15; i++) {
    await page.waitForTimeout(3000);

    const state = await page.evaluate(() => {
      const v = document.querySelector("video");
      const statusCard = document.body.innerText.includes("Stream Status");
      const peersText = document.body.innerText.match(/Peers:\s*(\d+)/);
      const speedText = document.body.innerText.match(/Speed:\s*([\d.]+)/);
      const progressText = document.body.innerText.match(/([\d.]+)%/);
      const waitingVisible = document.body.innerText.includes("Waiting for file") || document.body.innerText.includes("Waiting for stream");
      const connectingVisible = document.body.innerText.includes("Connecting");
      return {
        hasVideo: !!v,
        currentTime: v?.currentTime ?? 0,
        readyState: v?.readyState ?? 0,
        paused: v?.paused ?? true,
        statusCard,
        peers: peersText ? peersText[1] : null,
        speed: speedText ? speedText[1] : null,
        progress: progressText ? progressText[1] : null,
        waitingVisible,
        connectingVisible,
        bodySnippet: document.body.innerText.substring(0, 300),
      };
    });

    console.log(
      `[${i}] video=${state.hasVideo} time=${state.currentTime.toFixed(1)} ready=${state.readyState} ` +
        `statusCard=${state.statusCard} peers=${state.peers} speed=${state.speed} progress=${state.progress} ` +
        `waiting=${state.waitingVisible} connecting=${state.connectingVisible}`
    );

    // Verify no NaN appears in the UI
    expect(state.bodySnippet).not.toContain("NaN");

    if (state.statusCard) {
      streamStatusVisible = true;
      // Verify peers shows a number (not NaN)
      if (state.peers !== null) {
        const peersNum = parseInt(state.peers, 10);
        expect(Number.isNaN(peersNum)).toBe(false);
      }
      // Verify speed shows a number (not NaN)
      if (state.speed !== null) {
        const speedNum = parseFloat(state.speed);
        expect(Number.isNaN(speedNum)).toBe(false);
      }
    }

    if (state.hasVideo) {
      videoVisible = true;
      // Try to play if paused
      if (state.currentTime === 0) {
        await page.evaluate(() => {
          const v = document.querySelector("video");
          if (v) v.play().catch(() => {});
        });
        const playBtn = page.locator(".vjs-big-play-button");
        if (await playBtn.isVisible().catch(() => false)) {
          await playBtn.click().catch(() => {});
        }
        console.log("Triggered play");
      }

      if (state.currentTime > 0.5) {
        await page.screenshot({
          path: "test-results/torrent-03-playing.png",
        });
        console.log(
          "VIDEO IS PLAYING at",
          state.currentTime.toFixed(1),
          "seconds"
        );
        break;
      }
    }

    // If we have stream status visible AND video element, that's good enough
    if (streamStatusVisible && videoVisible) {
      await page.screenshot({
        path: "test-results/torrent-03-stream-status.png",
      });
      console.log("Stream status and video element both visible - success");
      break;
    }
  }

  await page.screenshot({ path: "test-results/torrent-04-final.png" });

  if (!videoVisible) {
    const recentLogs = logs.filter(
      (l) =>
        l.includes("error") || l.includes("Error") || l.includes("fail")
    );
    console.log("ERROR LOGS:", recentLogs.slice(-10));
  }
  expect(videoVisible).toBe(true);
});
