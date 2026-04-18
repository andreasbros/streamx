import { describe, it, expect, beforeEach } from "vitest";
import { getToken, setToken, removeToken, isTokenExpired } from "../src/lib/auth";

const localStorageMock = (() => {
  let store: Record<string, string> = {};
  return {
    getItem: (key: string) => store[key] ?? null,
    setItem: (key: string, value: string) => {
      store[key] = value;
    },
    removeItem: (key: string) => {
      delete store[key];
    },
    clear: () => {
      store = {};
    },
  };
})();

Object.defineProperty(globalThis, "localStorage", { value: localStorageMock });

describe("auth token management", () => {
  beforeEach(() => {
    localStorageMock.clear();
  });

  it("stores and retrieves a token", () => {
    setToken("abc123");
    expect(getToken()).toBe("abc123");
  });

  it("returns null when no token", () => {
    expect(getToken()).toBeNull();
  });

  it("removes token", () => {
    setToken("abc123");
    removeToken();
    expect(getToken()).toBeNull();
  });

  it("detects expired tokens", () => {
    const expiredPayload = btoa(JSON.stringify({ exp: Math.floor(Date.now() / 1000) - 100 }));
    const token = `header.${expiredPayload}.signature`;
    expect(isTokenExpired(token)).toBe(true);
  });

  it("detects valid tokens", () => {
    const validPayload = btoa(JSON.stringify({ exp: Math.floor(Date.now() / 1000) + 3600 }));
    const token = `header.${validPayload}.signature`;
    expect(isTokenExpired(token)).toBe(false);
  });
});
