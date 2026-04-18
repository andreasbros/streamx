/**
 * @vitest-environment jsdom
 */
import { describe, it, expect, beforeEach, vi } from "vitest";
import { renderHook } from "@testing-library/react";

describe("useTheme", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.resetModules();
  });

  it("defaults to dark theme", async () => {
    const { useTheme } = await import("../src/hooks/useTheme");
    const { result } = renderHook(() => useTheme());
    expect(result.current.theme).toBe("dark");
  });

  it("reads stored theme preference", async () => {
    localStorage.setItem("streamx_theme", "light");
    const { useTheme } = await import("../src/hooks/useTheme");
    const { result } = renderHook(() => useTheme());
    expect(result.current.theme).toBe("light");
  });
});

describe("useDebug", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.resetModules();
  });

  it("defaults to false", async () => {
    const { useDebug } = await import("../src/hooks/useDebug");
    const { result } = renderHook(() => useDebug());
    expect(result.current.debug).toBe(false);
  });
});
