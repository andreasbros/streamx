import { describe, it, expect, beforeEach, vi } from "vitest";
import { debugLog } from "../src/lib/debug-log";

describe("debugLog", () => {
  beforeEach(() => {
    debugLog.clear();
    vi.restoreAllMocks();
  });

  it("logs entries at different levels", () => {
    vi.spyOn(console, "log").mockImplementation(() => {});
    vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.spyOn(console, "error").mockImplementation(() => {});

    debugLog.info("test", "info message");
    debugLog.warn("test", "warn message");
    debugLog.error("test", "error message");
    debugLog.debug("test", "debug message");

    const entries = debugLog.getEntries();
    expect(entries).toHaveLength(4);
    expect(entries[0]?.level).toBe("info");
    expect(entries[1]?.level).toBe("warn");
    expect(entries[2]?.level).toBe("error");
    expect(entries[3]?.level).toBe("debug");
  });

  it("notifies subscribers", () => {
    vi.spyOn(console, "log").mockImplementation(() => {});
    const callback = vi.fn();
    const unsubscribe = debugLog.subscribe(callback);

    debugLog.info("test", "hello");
    expect(callback).toHaveBeenCalledTimes(1);

    unsubscribe();
    debugLog.info("test", "world");
    expect(callback).toHaveBeenCalledTimes(1);
  });

  it("clears entries", () => {
    vi.spyOn(console, "log").mockImplementation(() => {});
    debugLog.info("test", "message");
    expect(debugLog.getEntries()).toHaveLength(1);
    debugLog.clear();
    expect(debugLog.getEntries()).toHaveLength(0);
  });
});
