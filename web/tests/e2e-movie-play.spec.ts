import { test, expect } from "@playwright/test";

async function getToken(request: any): Promise<string> {
  const res = await request.post("/api/auth/login", {
    data: { username: "admin", password: "password" },
  });
  const body = await res.json();
  return body.token;
}

test.describe("Movie playback e2e", () => {
  test("demo Big Buck Bunny plays video frames", async ({ page, request }) => {
    test.setTimeout(30000);

    await page.goto("/login");
    await page.fill('input[placeholder*="ser"]', "admin");
    await page.fill('input[placeholder*="ass"]', "password");
    await page.click('button[type="submit"]');
    await page.waitForURL("**/", { timeout: 10000 });

    await page.goto("/player/demo");
    await page.waitForTimeout(3000);

    // Click our custom play overlay (covers the video box)
    const overlay = page.locator('[style*="cursor: pointer"]').first();
    if (await overlay.isVisible().catch(() => false)) {
      await overlay.click();
    }

    const played = await page.evaluate(() => {
      return new Promise<{ currentTime: number; paused: boolean; readyState: number }>((resolve) => {
        const check = () => {
          const v = document.querySelector("video");
          if (v && v.currentTime > 1) {
            resolve({ currentTime: v.currentTime, paused: v.paused, readyState: v.readyState });
          } else {
            setTimeout(check, 500);
          }
        };
        check();
        setTimeout(() => {
          const v = document.querySelector("video");
          resolve({
            currentTime: v?.currentTime ?? 0,
            paused: v?.paused ?? true,
            readyState: v?.readyState ?? 0,
          });
        }, 15000);
      });
    });

    console.log("Video state:", JSON.stringify(played));
    expect(played.currentTime).toBeGreaterThan(1);
    expect(played.paused).toBe(false);

    await page.screenshot({ path: "test-results/demo-verified-playing.png" });
  });

  test("torrent movie downloads segments and plays", async ({ request }) => {
    test.setTimeout(180000);

    const token = await getToken(request);
    const headers = { Authorization: `Bearer ${token}` };

    const searchRes = await request.post("/api/search", {
      headers,
      data: { query: "night of the living dead 1968" },
    });
    const searchData = await searchRes.json();
    const groups = searchData.results;
    expect(groups.length).toBeGreaterThan(0);

    // Results are grouped - pick the first variant from the best group
    const group = groups.find(
      (g: any) => g.title.includes("1968") && g.variants?.length > 0
    ) || groups[0];
    const variant = group.variants[0];

    const streamRes = await request.post("/api/stream", {
      headers,
      data: { magnet_uri: variant.magnet },
    });
    expect(streamRes.ok()).toBeTruthy();
    const { stream_id } = await streamRes.json();
    console.log(`Stream started: ${stream_id}`);

    let hasSegments = false;
    for (let i = 0; i < 60; i++) {
      await new Promise((r) => setTimeout(r, 3000));

      const statusRes = await request.get(`/api/stream/${stream_id}`, { headers });
      if (!statusRes.ok()) continue;
      const status = await statusRes.json();
      console.log(
        `[${i}] progress=${status.progress.toFixed(1)}% peers=${status.peers} speed=${status.speed} status=${status.status}`
      );

      const playlistRes = await request.get(
        `/api/stream/${stream_id}/playlist.m3u8?token=${token}`
      );
      if (playlistRes.ok()) {
        const playlist = await playlistRes.text();
        if (playlist.includes("segment_")) {
          console.log("HLS segments detected in playlist");
          hasSegments = true;

          const lines = playlist.split("\n");
          const segmentLine = lines.find((l: string) => l.includes("segment_"));
          if (segmentLine) {
            const segRes = await request.get(
              `/api/stream/${stream_id}/${segmentLine.trim()}?token=${token}`
            );
            expect(segRes.ok()).toBeTruthy();
            const segBody = await segRes.body();
            console.log(`Segment ${segmentLine.trim()} size: ${segBody.length} bytes`);
            expect(segBody.length).toBeGreaterThan(100);
          }
          break;
        }
      }
    }

    expect(hasSegments).toBe(true);

    await request.delete(`/api/stream/${stream_id}`, { headers });
  });
});
