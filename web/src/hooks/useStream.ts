import { useState, useEffect, useRef } from "react";
import type { StreamStatus, StreamMetadata } from "../api/types";
import { debugLog } from "../lib/debug-log";

export function useStream(streamId: string | null) {
  const [status, setStatus] = useState<StreamStatus | null>(null);
  const [fileUrl, setFileUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [metadata, setMetadata] = useState<StreamMetadata | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const retryRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const aliveRef = useRef(false);

  useEffect(() => {
    if (!streamId) {
      setStatus(null);
      setFileUrl(null);
      setError(null);
      return;
    }

    let cancelled = false;
    const fileEndpoint = `/api/stream/${streamId}/file`;

    const connect = () => {
      if (cancelled) {
        debugLog.warn("ws", "connect skipped: cancelled");
        return;
      }
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }

      const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
      const wsUrl = `${protocol}//${window.location.host}/api/stream/${streamId}/ws`;
      debugLog.info("ws", "connecting", wsUrl);

      const ws = new WebSocket(wsUrl);
      wsRef.current = ws;
      aliveRef.current = false;

      ws.onopen = () => {
        debugLog.info("ws", "open");
        aliveRef.current = true;
        setError(null);
      };

      ws.onmessage = (event) => {
        try {
          const msg = JSON.parse(event.data);
          debugLog.debug("ws", `msg: ${msg.type} ${msg.type === "status" ? msg.data.status : ""}`);
          if (msg.type === "status") {
            if (!document.fullscreenElement) {
              setStatus(msg.data);
              setError(null);
            }
            if (msg.data.status === "complete" || msg.data.status === "ready") {
              setFileUrl((prev) => {
                if (!prev) debugLog.info("ws", "fileUrl set (status)");
                return prev ?? fileEndpoint;
              });
            }
          } else if (msg.type === "file_ready") {
            setFileUrl(msg.data.url);
          } else if (msg.type === "metadata") {
            setMetadata(msg.data);
          } else if (msg.type === "error") {
            setError(msg.data.message);
          }
        } catch { /* ignore */ }
      };

      ws.onerror = () => debugLog.error("ws", "error");

      ws.onclose = (e) => {
        debugLog.warn("ws", `close code=${e.code} cancelled=${cancelled}`);
        wsRef.current = null;
        if (!cancelled) {
          debugLog.info("ws", "retry in 2s");
          retryRef.current = setTimeout(connect, 2000);
        }
      };
    };

    const watchdog = setInterval(() => {
      if (cancelled) return;
      const ws = wsRef.current;
      const dead = !ws || ws.readyState === WebSocket.CLOSED || ws.readyState === WebSocket.CLOSING;
      const stuck = ws && ws.readyState === WebSocket.CONNECTING && !aliveRef.current;
      if ((dead || stuck) && !retryRef.current) {
        debugLog.warn("ws", `watchdog: ${dead ? "dead" : "stuck"}, reconnecting`);
        connect();
      }
    }, 3000);

    const onVisible = () => {
      if (document.visibilityState !== "visible" || cancelled) return;
      debugLog.info("ws", "visibility: reconnecting");
      if (retryRef.current) { clearTimeout(retryRef.current); retryRef.current = null; }
      if (wsRef.current) { wsRef.current.close(); wsRef.current = null; }
      connect();
    };

    connect();
    document.addEventListener("visibilitychange", onVisible);

    return () => {
      cancelled = true;
      clearInterval(watchdog);
      document.removeEventListener("visibilitychange", onVisible);
      if (retryRef.current) clearTimeout(retryRef.current);
      if (wsRef.current) { wsRef.current.close(); wsRef.current = null; }
    };
  }, [streamId]);

  return { status, fileUrl, error, metadata };
}
