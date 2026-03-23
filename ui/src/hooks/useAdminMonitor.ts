import { useState, useEffect, useRef } from "react";

export interface DiskStats {
  total_bytes: number;
  free_bytes: number;
  cache_bytes: number;
  downloads_bytes: number;
}

export interface ProcessStats {
  rss_bytes: number;
  cpu_percent: number;
  ffmpeg_count: number;
}

export interface UserStats {
  active_connections: number;
}

export interface ActiveStream {
  stream_id: string;
  quality: string;
  status: string;
  title: string;
  file_size: number;
  cache_bytes: number;
  last_activity: string;
}

export interface ActiveDownload {
  stream_id: string;
  title: string;
  file_name: string;
  file_size: number;
  progress: number;
  speed: number;
  peers: number;
  status: string;
  created_at: string;
  updated_at: string;
}

export interface SystemStats {
  disk: DiskStats;
  process: ProcessStats;
  users: UserStats;
  streams: ActiveStream[];
  downloads: ActiveDownload[];
}

export function useAdminMonitor() {
  const [stats, setStats] = useState<SystemStats | null>(null);
  const [connected, setConnected] = useState(false);
  const wsRef = useRef<WebSocket | null>(null);

  useEffect(() => {
    let cancelled = false;
    let retryTimeout: ReturnType<typeof setTimeout> | null = null;

    const connect = () => {
      if (cancelled) return;
      const token = localStorage.getItem("streamx_token") || "";
      const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
      const url = `${protocol}//${window.location.host}/api/admin/monitor?token=${encodeURIComponent(token)}`;

      const ws = new WebSocket(url);
      wsRef.current = ws;

      ws.onopen = () => { if (!cancelled) setConnected(true); };
      ws.onmessage = (event) => {
        if (cancelled) return;
        try { setStats(JSON.parse(event.data)); } catch { /* ignore */ }
      };
      ws.onclose = () => {
        wsRef.current = null;
        if (!cancelled) {
          setConnected(false);
          retryTimeout = setTimeout(connect, 3000);
        }
      };
      ws.onerror = () => {};
    };

    connect();
    return () => {
      cancelled = true;
      if (retryTimeout) clearTimeout(retryTimeout);
      if (wsRef.current) { wsRef.current.close(); wsRef.current = null; }
    };
  }, []);

  return { stats, connected };
}

export interface LogEntry {
  seq: number;
  ts: string;
  level: string;
  target: string;
  message: string;
}

export function useAdminLogs(active: boolean) {
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [connected, setConnected] = useState(false);
  const wsRef = useRef<WebSocket | null>(null);
  const lastSeqRef = useRef<number>(0);
  const maxLogs = 2000;

  useEffect(() => {
    if (!active) {
      if (wsRef.current) { wsRef.current.close(); wsRef.current = null; }
      setConnected(false);
      return;
    }

    let cancelled = false;
    let retryTimeout: ReturnType<typeof setTimeout> | null = null;

    const connect = () => {
      if (cancelled) return;
      const token = localStorage.getItem("streamx_token") || "";
      const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
      const afterSeq = lastSeqRef.current;
      const url = `${protocol}//${window.location.host}/api/admin/logs?token=${encodeURIComponent(token)}&after=${afterSeq}`;

      const ws = new WebSocket(url);
      wsRef.current = ws;

      ws.onopen = () => { if (!cancelled) setConnected(true); };
      ws.onmessage = (event) => {
        if (cancelled) return;
        try {
          const entry: LogEntry = JSON.parse(event.data);
          if (entry.seq > lastSeqRef.current) {
            lastSeqRef.current = entry.seq;
            setLogs((prev) => {
              const next = [...prev, entry];
              return next.length > maxLogs ? next.slice(next.length - maxLogs) : next;
            });
          }
        } catch { /* ignore */ }
      };
      ws.onclose = () => {
        wsRef.current = null;
        if (!cancelled) {
          setConnected(false);
          retryTimeout = setTimeout(connect, 3000);
        }
      };
      ws.onerror = () => {};
    };

    connect();
    return () => {
      cancelled = true;
      if (retryTimeout) clearTimeout(retryTimeout);
      if (wsRef.current) { wsRef.current.close(); wsRef.current = null; }
    };
  }, [active]);

  const clear = () => setLogs([]);

  return { logs, connected, clear };
}
