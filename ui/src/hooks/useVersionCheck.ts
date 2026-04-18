import { useState, useEffect, useRef } from "react";

let currentHash: string | null = null;

export function useVersionCheck() {
  const [updateAvailable, setUpdateAvailable] = useState(false);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    // Fetch initial version
    fetch("/api/version")
      .then((r) => r.json())
      .then((data) => {
        currentHash = data.hash;
      })
      .catch(() => {});

    // Poll every 60 seconds
    intervalRef.current = setInterval(() => {
      fetch("/api/version")
        .then((r) => r.json())
        .then((data) => {
          if (currentHash && data.hash !== currentHash) {
            setUpdateAvailable(true);
          }
        })
        .catch(() => {});
    }, 60000);

    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, []);

  const reload = () => {
    window.location.reload();
  };

  return { updateAvailable, reload };
}
