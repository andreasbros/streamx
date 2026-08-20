import { useState, useRef, useEffect, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import {
  Flex,
  Text,
  Card,
  Badge,
  Button,
  Switch,
  TextField,
} from "@radix-ui/themes";
import { ArrowLeftIcon, PlayIcon, Cross2Icon, MagnifyingGlassIcon, ChevronDownIcon, ChevronUpIcon, CopyIcon, TrashIcon } from "@radix-ui/react-icons";
import { useAuth } from "../hooks/useAuth";
import { useAdminMonitor, useAdminLogs } from "../hooks/useAdminMonitor";
import type { SystemStats, LogEntry } from "../hooks/useAdminMonitor";
import { formatBytes, formatSpeed } from "../lib/utils";
import { getToken } from "../lib/auth";
import { api } from "../api/client";
import { invalidateServerSettings, useServerSettings } from "../hooks/useServerSettings";
import type { ServerSettings } from "../api/types";

function formatTimeAgo(iso: string): string {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  const now = Date.now();
  const diff = now - d.getTime();
  if (diff < 60_000) return "just now";
  if (diff < 3600_000) return `${Math.floor(diff / 60_000)}m ago`;
  if (diff < 86400_000) return `${Math.floor(diff / 3600_000)}h ago`;
  return `${Math.floor(diff / 86400_000)}d ago`;
}

function StatRow({ label, value, color }: { label: string; value: string; color?: "green" | "amber" | "red" | "blue" | "gray" }) {
  return (
    <Flex align="center" justify="between">
      <Text size="2" color="gray">{label}</Text>
      <Text size="2" weight="medium" color={color}>{value}</Text>
    </Flex>
  );
}

function Bar({ pct, color }: { pct: number; color?: string }) {
  const c = color || (pct > 90 ? "var(--red-9)" : pct > 70 ? "var(--amber-9)" : "var(--green-9)");
  return (
    <div style={{ height: 6, borderRadius: 3, overflow: "hidden", background: "var(--gray-a3)" }}>
      <div style={{ height: "100%", width: `${Math.min(pct, 100)}%`, background: c, borderRadius: 3, transition: "width 0.3s" }} />
    </div>
  );
}

function DiskCard({ stats }: { stats: SystemStats }) {
  const used = stats.disk.total_bytes - stats.disk.free_bytes;
  const pct = stats.disk.total_bytes > 0 ? (used / stats.disk.total_bytes) * 100 : 0;
  return (
    <Card>
      <Flex direction="column" gap="2">
        <Text size="3" weight="medium">Disk</Text>
        <Bar pct={pct} />
        <StatRow label="Used / Total" value={`${formatBytes(used)} / ${formatBytes(stats.disk.total_bytes)}`} />
        <StatRow label="Free" value={formatBytes(stats.disk.free_bytes)} color="green" />
        <StatRow label="HLS Cache" value={formatBytes(stats.disk.cache_bytes)} color="blue" />
        <StatRow label="Downloads" value={formatBytes(stats.disk.downloads_bytes)} />
      </Flex>
    </Card>
  );
}

function ProcessCard({ stats }: { stats: SystemStats }) {
  return (
    <Card>
      <Flex direction="column" gap="2">
        <Text size="3" weight="medium">Process</Text>
        <StatRow label="Memory (RSS)" value={formatBytes(stats.process.rss_bytes)} />
        <StatRow
          label="CPU"
          value={`${stats.process.cpu_percent.toFixed(1)}%`}
          color={stats.process.cpu_percent > 80 ? "red" : stats.process.cpu_percent > 50 ? "amber" : "green"}
        />
        <StatRow
          label="FFmpeg processes"
          value={String(stats.process.ffmpeg_count)}
          color={stats.process.ffmpeg_count > 0 ? "blue" : "gray"}
        />
      </Flex>
    </Card>
  );
}

function UsersCard({ stats }: { stats: SystemStats }) {
  return (
    <Card>
      <Flex direction="column" gap="2">
        <Text size="3" weight="medium">Users</Text>
        <StatRow
          label="Active connections"
          value={String(stats.users.active_connections)}
          color={stats.users.active_connections > 0 ? "green" : "gray"}
        />
      </Flex>
    </Card>
  );
}

function TranscodesCard({ stats }: { stats: SystemStats }) {
  const navigate = useNavigate();
  const [showCount, setShowCount] = useState(5);

  if (stats.streams.length === 0) return null;

  const visible = stats.streams.slice(0, showCount);
  const hasMore = showCount < stats.streams.length;

  return (
    <Card>
      <Flex direction="column" gap="2">
        <Flex align="center" justify="between">
          <Text size="3" weight="medium">Transcodes</Text>
          <Badge size="1" variant="soft" color="gray">{stats.streams.length}</Badge>
        </Flex>
        {visible.map((s, i) => {
          const pct = s.file_size > 0 ? (s.cache_bytes / s.file_size) * 100 : 0;
          return (
            <Flex key={`${s.stream_id}-${s.quality}`} direction="column" gap="1" style={{ padding: "4px 0", borderBottom: i < visible.length - 1 ? "1px solid var(--gray-a3)" : undefined }}>
              <Flex align="center" gap="2" wrap="wrap">
                <Badge size="1" variant="solid" color={s.status === "running" ? "green" : s.status === "complete" ? "blue" : s.status === "cached" ? "gray" : "red"}>
                  {s.status}
                </Badge>
                <Badge size="1" variant="soft" color="gray">{s.quality}</Badge>
                <Text size="1" weight="medium" style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                  {s.title || s.stream_id.substring(0, 12)}
                </Text>
                <Button variant="ghost" size="1" onClick={() => navigate(`/player/${s.stream_id}`)}>
                  <PlayIcon width={12} height={12} />
                </Button>
                {(s.status === "running" || s.status === "cached") && (
                  <Button
                    variant="ghost"
                    size="1"
                    color="red"
                    onClick={() => {
                      const token = localStorage.getItem("streamx_token") || "";
                      fetch(`/api/admin/kill/${s.stream_id}`, {
                        method: "DELETE",
                        headers: { Authorization: `Bearer ${token}` },
                      }).catch(() => {});
                    }}
                  >
                    <TrashIcon width={12} height={12} />
                  </Button>
                )}
              </Flex>
              <Bar pct={pct} color={s.status === "running" ? "var(--green-9)" : "var(--blue-9)"} />
              <Flex justify="between" wrap="wrap" gap="1">
                <Text size="1" color="gray">
                  {formatBytes(s.cache_bytes)} / {formatBytes(s.file_size)} ({pct.toFixed(1)}%)
                </Text>
                {s.last_activity && (
                  <Text size="1" color="gray" title={s.last_activity}>
                    {formatTimeAgo(s.last_activity)}
                  </Text>
                )}
              </Flex>
            </Flex>
          );
        })}
        {hasMore && (
          <Button variant="ghost" size="1" onClick={() => setShowCount((c) => c + 5)} style={{ alignSelf: "center" }}>
            Show more ({stats.streams.length - showCount} remaining)
          </Button>
        )}
      </Flex>
    </Card>
  );
}

function statusColor(status: string): "amber" | "green" | "red" | "blue" | "gray" {
  switch (status) {
    case "downloading": return "amber";
    case "complete": return "green";
    case "error": return "red";
    case "initializing": return "blue";
    default: return "gray";
  }
}

function DownloadsCard({ stats }: { stats: SystemStats }) {
  const navigate = useNavigate();
  const [showCount, setShowCount] = useState(5);

  if (stats.downloads.length === 0) return null;

  const visible = stats.downloads.slice(0, showCount);
  const hasMore = showCount < stats.downloads.length;

  return (
    <Card>
      <Flex direction="column" gap="2">
        <Flex align="center" justify="between">
          <Text size="3" weight="medium">Downloads</Text>
          <Badge size="1" variant="soft" color="gray">{stats.downloads.length}</Badge>
        </Flex>
        {visible.map((dl, i) => (
          <Flex key={dl.stream_id} direction="column" gap="1" style={{ padding: "4px 0", borderBottom: i < visible.length - 1 ? "1px solid var(--gray-a3)" : undefined }}>
            <Flex align="center" gap="2" wrap="wrap">
              <Badge size="1" variant="solid" color={statusColor(dl.status)}>
                {dl.status}
              </Badge>
              <Text size="1" weight="medium" style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {dl.title || dl.file_name || dl.stream_id.substring(0, 16) + "..."}
              </Text>
              <Button variant="ghost" size="1" onClick={() => navigate(`/player/${dl.stream_id}`)}>
                <PlayIcon width={12} height={12} />
              </Button>
              <Button variant="ghost" size="1" color="red" onClick={() => {
                api.deleteStream(dl.stream_id).catch(() => {});
              }}>
                <TrashIcon width={12} height={12} />
              </Button>
            </Flex>
            <Bar pct={dl.progress} color={dl.status === "complete" ? "var(--green-9)" : "var(--amber-9)"} />
            <Flex justify="between" wrap="wrap" gap="1">
              <Text size="1" color="gray">
                {formatBytes(dl.file_size * dl.progress / 100)} / {formatBytes(dl.file_size)} ({dl.progress.toFixed(1)}%)
              </Text>
              <Flex gap="3">
                {(dl.status === "downloading" || dl.status === "initializing") && (
                  <>
                    <Text size="1" color="gray">Peers {dl.peers}</Text>
                    <Text size="1" color="gray">{formatSpeed(dl.speed)}</Text>
                  </>
                )}
                <Text size="1" color="gray" title={`Started: ${dl.created_at}\nUpdated: ${dl.updated_at}`}>
                  {formatTimeAgo(dl.created_at)}
                  {dl.status === "complete" && ` - ${formatTimeAgo(dl.updated_at)}`}
                </Text>
              </Flex>
            </Flex>
          </Flex>
        ))}
        {hasMore && (
          <Button variant="ghost" size="1" onClick={() => setShowCount((c) => c + 5)} style={{ alignSelf: "center" }}>
            Show more ({stats.downloads.length - showCount} remaining)
          </Button>
        )}
      </Flex>
    </Card>
  );
}

function ServerSettingsCard() {
  const settings = useServerSettings();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const toggle = async (patch: Partial<ServerSettings>) => {
    if (!settings) return;
    setSaving(true);
    setError(null);
    try {
      const next = { ...settings, ...patch };
      const saved = await api.updateServerSettings(next);
      invalidateServerSettings(saved);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Save failed");
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card size="2">
      <Flex direction="column" gap="3">
        <Text size="3" weight="bold">Playback & Search</Text>
        <Flex align="center" justify="between" gap="3">
          <Flex direction="column" gap="0" style={{ flex: 1 }}>
            <Text size="2">Disable server-side transcoding</Text>
            <Text size="1" color="gray">
              Non-WEB releases lose their Play button (still downloadable);
              WEB releases play directly in the browser without transcoding.
            </Text>
          </Flex>
          <Switch
            checked={settings?.disable_transcode ?? true}
            disabled={!settings || saving}
            onCheckedChange={(v) => toggle({ disable_transcode: v })}
          />
        </Flex>
        <Flex align="center" justify="between" gap="3">
          <Flex direction="column" gap="0" style={{ flex: 1 }}>
            <Text size="2">WEB releases only</Text>
            <Text size="1" color="gray">
              Search results and new downloads are restricted to WEB source releases.
            </Text>
          </Flex>
          <Switch
            checked={settings?.web_only ?? false}
            disabled={!settings || saving}
            onCheckedChange={(v) => toggle({ web_only: v })}
          />
        </Flex>
        {error && <Text size="2" color="red">{error}</Text>}
      </Flex>
    </Card>
  );
}

function MaintenanceCard() {
  const [busy, setBusy] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const restartTorrent = async () => {
    if (!window.confirm("Restart the torrent client? All peer connections close and DHT discovery starts fresh.")) return;
    setBusy("torrent");
    setMessage(null);
    try {
      const res = await fetch("/api/admin/restart-torrent", {
        method: "POST",
        headers: { Authorization: `Bearer ${getToken() ?? ""}` },
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const body = await res.json();
      setMessage(`Torrent client restarted (${body.readded ?? 0} downloads re-added).`);
    } catch (err) {
      setMessage(`Restart failed: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setBusy(null);
    }
  };

  const restartServer = async () => {
    if (!window.confirm("Restart the whole server? The app will briefly disconnect.")) return;
    setBusy("server");
    setMessage(null);
    try {
      const res = await fetch("/api/admin/restart-server", {
        method: "POST",
        headers: { Authorization: `Bearer ${getToken() ?? ""}` },
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      setMessage("Server restarting; the page will reconnect shortly.");
      setTimeout(() => window.location.reload(), 4000);
    } catch (err) {
      setMessage(`Restart failed: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setBusy(null);
    }
  };

  return (
    <Card size="2">
      <Flex direction="column" gap="3">
        <Text size="3" weight="bold">Maintenance</Text>
        <Flex gap="3" wrap="wrap" align="center">
          <Button
            variant="soft"
            color="orange"
            disabled={busy !== null}
            onClick={restartTorrent}
          >
            {busy === "torrent" ? "Restarting…" : "Restart Torrent Client"}
          </Button>
          <Button
            variant="soft"
            color="red"
            disabled={busy !== null}
            onClick={restartServer}
          >
            {busy === "server" ? "Restarting…" : "Restart Server"}
          </Button>
        </Flex>
        <Text size="1" color="gray">
          Torrent restart closes every peer connection and rediscovers seed nodes;
          active and background downloads are re-added automatically.
        </Text>
        {message && <Text size="2">{message}</Text>}
      </Flex>
    </Card>
  );
}

function levelColor(level: string): string {
  switch (level) {
    case "ERROR": return "var(--red-11)";
    case "WARN": return "var(--amber-11)";
    case "INFO": return "var(--blue-11)";
    case "DEBUG": return "var(--gray-9)";
    default: return "var(--gray-11)";
  }
}

function LogViewer() {
  const [expanded, setExpanded] = useState(false);
  const { logs, connected, clear } = useAdminLogs(expanded);
  const [search, setSearch] = useState("");
  const [levelFilter, setLevelFilter] = useState<string>("ALL");
  const containerRef = useRef<HTMLDivElement>(null);

  const filtered = logs.filter((l) => {
    if (levelFilter !== "ALL" && l.level !== levelFilter) return false;
    if (search) {
      const q = search.toLowerCase();
      return l.message.toLowerCase().includes(q) || l.target.toLowerCase().includes(q);
    }
    return true;
  });

  const [tailing, setTailing] = useState(true);
  const rafRef = useRef<number>(0);
  const programmaticScroll = useRef(false);

  useEffect(() => {
    if (!tailing) return;
    cancelAnimationFrame(rafRef.current);
    rafRef.current = requestAnimationFrame(() => {
      const el = containerRef.current;
      if (el) {
        programmaticScroll.current = true;
        el.scrollTop = el.scrollHeight;
        requestAnimationFrame(() => { programmaticScroll.current = false; });
      }
    });
    return () => cancelAnimationFrame(rafRef.current);
  }, [filtered.length, tailing]);

  const handleScroll = useCallback(() => {
    if (programmaticScroll.current) return;
    if (tailing) setTailing(false);
  }, [tailing]);

  return (
    <Card>
      <Flex direction="column" gap="2">
        <Flex
          align="center"
          justify="between"
          onClick={() => setExpanded((v) => !v)}
          style={{ cursor: "pointer" }}
        >
          <Flex align="center" gap="2">
            <Text size="3" weight="medium">Logs</Text>
            {expanded && (
              <Badge size="1" variant="soft" color={connected ? "green" : "red"}>
                {connected ? "Streaming" : "Disconnected"}
              </Badge>
            )}
            {expanded && <Badge size="1" variant="soft" color="gray">{filtered.length}</Badge>}
          </Flex>
          {expanded ? <ChevronUpIcon /> : <ChevronDownIcon />}
        </Flex>

        {expanded && (
          <>
            <Flex gap="2" align="center">
              <select
                value={levelFilter}
                onChange={(e) => setLevelFilter(e.target.value)}
                style={{
                  background: "var(--gray-a3)",
                  color: "inherit",
                  border: "1px solid var(--gray-a5)",
                  borderRadius: 4,
                  padding: "2px 6px",
                  fontSize: 12,
                  cursor: "pointer",
                  flexShrink: 0,
                }}
              >
                <option value="ALL">All</option>
                <option value="ERROR">Error</option>
                <option value="WARN">Warn</option>
                <option value="INFO">Info</option>
                <option value="DEBUG">Debug</option>
                <option value="TRACE">Trace</option>
              </select>
              <TextField.Root
                size="1"
                placeholder="Search logs..."
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                style={{ flex: 1 }}
              >
                <TextField.Slot>
                  <MagnifyingGlassIcon width={12} height={12} />
                </TextField.Slot>
              </TextField.Root>
              <Button
                variant={tailing ? "solid" : "ghost"}
                size="1"
                color={tailing ? "blue" : "gray"}
                title="Tail (auto-scroll to latest)"
                onClick={() => setTailing(true)}
              >
                <ChevronDownIcon width={12} height={12} />
              </Button>
              <Button variant="ghost" size="1" title="Copy all" onClick={() => {
                const text = filtered.map((l) => `${l.ts} ${l.level.padEnd(5)} ${l.target} ${l.message}`).join("\n");
                navigator.clipboard.writeText(text).catch(() => {});
              }}>
                <CopyIcon width={12} height={12} />
              </Button>
              <Button variant="ghost" size="1" title="Clear" onClick={clear}>
                <Cross2Icon width={12} height={12} />
              </Button>
            </Flex>

            <div
              ref={containerRef}
              onScroll={handleScroll}
              style={{
                height: 400,
                overflow: "auto",
                fontFamily: "monospace",
                fontSize: 11,
                lineHeight: 1.5,
                background: "var(--gray-a2)",
                borderRadius: 4,
                padding: 8,
              }}
            >
              {filtered.map((log, i) => (
                <LogLine key={i} log={log} />
              ))}
              {filtered.length === 0 && (
                <Text size="1" color="gray">{search ? "No matching logs" : "Waiting for logs..."}</Text>
              )}
            </div>
          </>
        )}
      </Flex>
    </Card>
  );
}

function LogLine({ log }: { log: LogEntry }) {
  const time = log.ts.substring(11, 23);
  const isError = log.level === "ERROR";
  const isWarn = log.level === "WARN";
  return (
    <div style={{
      marginBottom: 1,
      whiteSpace: "pre-wrap",
      wordBreak: "break-word",
      padding: "1px 4px",
      borderRadius: 2,
      background: isError ? "rgba(229,62,62,0.1)" : isWarn ? "rgba(217,119,6,0.07)" : undefined,
      borderLeft: isError ? "2px solid var(--red-9)" : isWarn ? "2px solid var(--amber-9)" : "2px solid transparent",
    }}>
      <span style={{ color: "var(--gray-8)" }}>{time}</span>{" "}
      <span style={{
        color: levelColor(log.level),
        fontWeight: isError || isWarn ? 600 : 400,
      }}>{log.level.padEnd(5)}</span>{" "}
      <span style={{ color: "var(--blue-9)", opacity: 0.7 }}>{log.target}</span>{" "}
      <span style={{ color: isError ? "var(--red-11)" : isWarn ? "var(--amber-11)" : "var(--gray-12)" }}>{log.message}</span>
    </div>
  );
}

export function Admin() {
  const navigate = useNavigate();
  const { user } = useAuth();
  const { stats, connected } = useAdminMonitor();

  if (!user?.is_admin) {
    return (
      <Flex direction="column" align="center" gap="4" py="9">
        <Text size="4" color="gray">Admin access required</Text>
        <Button variant="soft" onClick={() => navigate("/")}>Go Home</Button>
      </Flex>
    );
  }

  return (
    <Flex direction="column" gap="4">
      <Flex align="center" gap="3">
        <Button variant="ghost" size="1" onClick={() => navigate(-1)}>
          <ArrowLeftIcon width={18} height={18} />
        </Button>
        <Text size="5" weight="bold">Admin</Text>
        <Badge size="1" variant="soft" color={connected ? "green" : "red"}>
          {connected ? "Live" : "Disconnected"}
        </Badge>
      </Flex>

      {stats ? (
        <>
          {/* Responsive grid: 1 col mobile, 2-3 cols desktop */}
          <div style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fit, minmax(280px, 1fr))",
            gap: 12,
          }}>
            <DiskCard stats={stats} />
            <ProcessCard stats={stats} />
            <UsersCard stats={stats} />
          </div>

          <TranscodesCard stats={stats} />
          <DownloadsCard stats={stats} />
          <ServerSettingsCard />
          <MaintenanceCard />
          <LogViewer />
        </>
      ) : (
        <Text size="2" color="gray">Connecting...</Text>
      )}
    </Flex>
  );
}
