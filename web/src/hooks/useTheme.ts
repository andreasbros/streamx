import { useState, useCallback } from "react";

type ThemeValue = "dark" | "light";
const STORAGE_KEY = "streamx_theme";

function getStoredTheme(): ThemeValue {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored === "light" || stored === "dark") return stored;
  return "dark";
}

export function useTheme() {
  const [theme, setThemeState] = useState<ThemeValue>(getStoredTheme);

  const setTheme = useCallback((value: ThemeValue) => {
    localStorage.setItem(STORAGE_KEY, value);
    setThemeState(value);
  }, []);

  const toggleTheme = useCallback(() => {
    setTheme(theme === "dark" ? "light" : "dark");
  }, [theme, setTheme]);

  return { theme, setTheme, toggleTheme } as const;
}
