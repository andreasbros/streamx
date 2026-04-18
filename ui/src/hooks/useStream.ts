import { useState, useEffect, useRef } from "react";
import type { StreamStatus } from "../api/types";

export function useStream(streamId: string | null) {
  const [status, setStatus] = useState<StreamStatus | null>(null);
  const [fileUrl, setFileUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const wsRef = useRef<WebSocket | null>(null);

  useEffect(() => {
    if (!streamId) {
      setStatus(null);
      setFileUrl(null);
      setError(null);
      return;
    }

    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    const wsUrl = `${protocol}//${window.location.host}/api/stream/${streamId}/ws`;
    const ws = new WebSocket(wsUrl);
    wsRef.current = ws;

    ws.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data);
        switch (msg.type) {
          case "status":
            setStatus(msg.data);
            break;
          case "file_ready":
            setFileUrl(msg.data.url);
            break;
          case "error":
            setError(msg.data.message);
            break;
        }
      } catch {
        // ignore malformed messages
      }
    };

    ws.onerror = () => setError("WebSocket connection failed");
    ws.onclose = () => {};

    return () => {
      ws.close();
      wsRef.current = null;
    };
  }, [streamId]);

  return { status, fileUrl, error };
}
