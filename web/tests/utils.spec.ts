import { describe, it, expect } from "vitest";
import {
  formatBytes,
  formatDuration,
  formatSpeed,
  isMagnetLink,
  detectQuality,
  classNames,
} from "../src/lib/utils";

describe("formatBytes", () => {
  it("formats zero bytes", () => {
    expect(formatBytes(0)).toBe("0 B");
  });

  it("formats megabytes", () => {
    expect(formatBytes(1048576)).toBe("1.0 MB");
  });

  it("formats gigabytes", () => {
    expect(formatBytes(1073741824)).toBe("1.0 GB");
  });
});

describe("formatDuration", () => {
  it("formats seconds only", () => {
    expect(formatDuration(45)).toBe("0:45");
  });

  it("formats minutes and seconds", () => {
    expect(formatDuration(125)).toBe("2:05");
  });

  it("formats hours", () => {
    expect(formatDuration(3661)).toBe("1:01:01");
  });
});

describe("formatSpeed", () => {
  it("formats bytes per second", () => {
    expect(formatSpeed(1048576)).toBe("1.0 MB/s");
  });
});

describe("isMagnetLink", () => {
  it("detects magnet links", () => {
    expect(isMagnetLink("magnet:?xt=urn:btih:abc")).toBe(true);
  });

  it("rejects non-magnet text", () => {
    expect(isMagnetLink("hello world")).toBe(false);
  });

  it("handles whitespace", () => {
    expect(isMagnetLink("  magnet:?xt=urn:btih:abc")).toBe(true);
  });
});

describe("detectQuality", () => {
  it("detects 1080p", () => {
    expect(detectQuality("Movie.2024.1080p.BluRay")).toBe("1080p");
  });

  it("detects 4K", () => {
    expect(detectQuality("Movie.2024.2160p.WEB")).toBe("4K");
  });

  it("detects 720p", () => {
    expect(detectQuality("Movie.720p")).toBe("720p");
  });

  it("returns null for unknown quality", () => {
    expect(detectQuality("Movie.2024")).toBeNull();
  });
});

describe("classNames", () => {
  it("joins truthy strings", () => {
    expect(classNames("a", false, "b", null, "c")).toBe("a b c");
  });
});
