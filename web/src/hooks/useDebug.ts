import { useCallback, useSyncExternalStore } from "react";

const STORAGE_KEY = "streamx_debug";

const listeners = new Set<() => void>();

function subscribe(cb: () => void) {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

function getSnapshot(): boolean {
  return localStorage.getItem(STORAGE_KEY) === "true";
}

function notify() {
  for (const cb of listeners) cb();
}

export function useDebug() {
  const debug = useSyncExternalStore(subscribe, getSnapshot);

  const setDebug = useCallback((value: boolean) => {
    localStorage.setItem(STORAGE_KEY, String(value));
    notify();
  }, []);

  return { debug, setDebug } as const;
}
