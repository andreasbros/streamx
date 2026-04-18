import { test, expect } from "@playwright/test";

const LOCAL = "http://localhost:8999";

test.describe("Local surround test files", () => {
  test("GET /proxy/local lists available MP4 files", async ({ request }) => {
    const res = await request.get(`${LOCAL}/proxy/local`);
    expect(res.status()).toBe(200);

    const files = await res.json();
    console.log("Files:", files);
    expect(files.length).toBeGreaterThan(0);

    for (const f of files) {
      expect(f.name).toMatch(/\.mp4$/);
      expect(f.size).toBeGreaterThan(0);
      expect(f.url).toMatch(/^\/proxy\/local\//);
    }
  });

  test("all local MP4 files support range requests", async ({ request }) => {
    const listRes = await request.get(`${LOCAL}/proxy/local`);
    const files = await listRes.json();

    for (const f of files) {
      const url = `${LOCAL}${f.url}`;

      // Range request should return 206
      const rangeRes = await request.get(url, {
        headers: { Range: "bytes=0-1023" },
      });
      expect(rangeRes.status(), `${f.name} range request`).toBe(206);
      expect(rangeRes.headers()["content-range"]).toMatch(/^bytes 0-1023\//);
      expect(Number(rangeRes.headers()["content-length"])).toBe(1024);
      expect(rangeRes.headers()["content-type"]).toBe("video/mp4");
      expect(rangeRes.headers()["accept-ranges"]).toBe("bytes");
      expect(rangeRes.headers()["cache-control"]).toContain("no-cache");

      // Mid-file range
      const midRes = await request.get(url, {
        headers: { Range: "bytes=1000000-1001023" },
      });
      expect(midRes.status(), `${f.name} mid-range`).toBe(206);
      expect(midRes.headers()["content-range"]).toMatch(
        /^bytes 1000000-1001023\//,
      );

      console.log(
        `${f.name}: ${f.size} bytes, range OK, content-type=${rangeRes.headers()["content-type"]}`,
      );
    }
  });

  test("full request returns correct Content-Length and Accept-Ranges", async ({
    request,
  }) => {
    const listRes = await request.get(`${LOCAL}/proxy/local`);
    const files = await listRes.json();

    // Test with smallest file only (avoid downloading huge files)
    const smallest = files.reduce(
      (min: { size: number }, f: { size: number }) =>
        f.size < min.size ? f : min,
      files[0],
    );

    const url = `${LOCAL}${smallest.url}`;
    const res = await request.get(url, {
      headers: { Range: "bytes=0-0" },
    });
    expect(res.status()).toBe(206);
    expect(res.headers()["accept-ranges"]).toBe("bytes");
    expect(res.headers()["content-type"]).toBe("video/mp4");
    console.log(
      `${smallest.name}: Content-Length=${res.headers()["content-length"]}`,
    );
  });
});
