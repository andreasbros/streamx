import { useState, useCallback } from "react";

const STORAGE_KEY = "streamx_debug";

function getStored(): boolean {
  return localStorage.getItem(STORAGE_KEY) === "true";
}

export function useDebug() {
  const [debug, setDebugState] = useState(getStored);

  const setDebug = useCallback((value: boolean) => {
    localStorage.setItem(STORAGE_KEY, String(value));
    setDebugState(value);
  }, []);

  return { debug, setDebug } as const;
}
