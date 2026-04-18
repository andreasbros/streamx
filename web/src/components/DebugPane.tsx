import { useState, useEffect, useRef, useCallback } from "react";
import {
  Flex,
  Text,
  Button,
  Badge,
  Select,
  Code,
  IconButton,
} from "@radix-ui/themes";
import { TrashIcon, Cross2Icon, ChevronUpIcon, CopyIcon } from "@radix-ui/react-icons";
import { debugLog } from "../lib/debug-log";
import type { LogLevel, LogEntry } from "../lib/debug-log";

const LEVEL_COLORS: Record<LogLevel, "gray" | "amber" | "red" | "blue"> = {
  debug: "gray",
  info: "blue",
  warn: "amber",
  error: "red",
};

function LogLine({ entry }: { entry: LogEntry }) {
  const time = new Date(entry.timestamp).toLocaleTimeString("en", {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
  return (
    <Flex gap="2" py="1" style={{ fontFamily: "var(--code-font-family)", fontSize: 11 }}>
      <Text size="1" color="gray" style={{ whiteSpace: "nowrap" }}>
        {time}
      </Text>
      <Badge size="1" color={LEVEL_COLORS[entry.level]} variant="soft">
        {entry.level.charAt(0).toUpperCase()}
      </Badge>
      <Text size="1" color="violet" style={{ whiteSpace: "nowrap" }}>
        {entry.source}
      </Text>
      <Text size="1" style={{ wordBreak: "break-all" }}>{entry.message}</Text>
      {entry.data !== undefined && (
        <Code size="1" color="gray" style={{ wordBreak: "break-all" }}>
          {typeof entry.data === "string" ? entry.data : JSON.stringify(entry.data)}
        </Code>
      )}
    </Flex>
  );
}

interface DebugPaneProps {
  onClose: () => void;
}

export function DebugPane({ onClose }: DebugPaneProps) {
  const [entries, setEntries] = useState<LogEntry[]>([]);
  const [filter, setFilter] = useState<LogLevel | "all">("all");
  const [expanded, setExpanded] = useState(true);
  const scrollRef = useRef<HTMLDivElement>(null);
  const autoScrollRef = useRef(true);

  const updateEntries = useCallback(() => {
    setEntries(debugLog.getEntries());
  }, []);

  useEffect(() => {
    updateEntries();
    return debugLog.subscribe(updateEntries);
  }, [updateEntries]);

  useEffect(() => {
    if (autoScrollRef.current && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [entries]);

  const handleScroll = () => {
    if (!scrollRef.current) return;
    const el = scrollRef.current;
    autoScrollRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
  };

  const filtered = filter === "all" ? entries : entries.filter((e) => e.level === filter);
  const errorCount = entries.filter((e) => e.level === "error").length;

  return (
    <div
      style={{
        position: "fixed",
        bottom: 0,
        left: 0,
        right: 0,
        zIndex: 999,
        background: "var(--color-panel-solid)",
        borderTop: "1px solid var(--gray-a5)",
      }}
    >
      <Flex
        align="center"
        justify="between"
        px="3"
        py="2"
        style={{ cursor: "pointer", userSelect: "none" }}
        onClick={() => setExpanded(!expanded)}
      >
        <Flex align="center" gap="2">
          <ChevronUpIcon
            style={{
              transform: expanded ? "rotate(180deg)" : "rotate(0)",
              transition: "transform 0.2s",
            }}
          />
          <Text size="2" weight="medium">Debug</Text>
          <Badge size="1" variant="soft">{entries.length}</Badge>
          {errorCount > 0 && (
            <Badge size="1" color="red" variant="solid">{errorCount} err</Badge>
          )}
          <IconButton variant="ghost" size="1" onClick={(e) => {
            e.stopPropagation();
            const text = entries.map((en) => {
              const t = new Date(en.timestamp).toLocaleTimeString("en", { hour12: false, hour: "2-digit", minute: "2-digit", second: "2-digit" });
              const d = en.data !== undefined ? ` ${typeof en.data === "string" ? en.data : JSON.stringify(en.data)}` : "";
              return `${t} ${en.level.charAt(0).toUpperCase()} ${en.source} ${en.message}${d}`;
            }).join("\n");
            navigator.clipboard.writeText(text).then(
              () => debugLog.info("debug", "Log copied to clipboard"),
              () => debugLog.warn("debug", "Copy failed")
            );
          }}>
            <CopyIcon width={12} height={12} />
          </IconButton>
        </Flex>
        <Flex align="center" gap="2" onClick={(e) => e.stopPropagation()}>
          {expanded && (
            <>
              <Select.Root
                size="1"
                value={filter}
                onValueChange={(v) => setFilter(v as LogLevel | "all")}
              >
                <Select.Trigger variant="ghost" />
                <Select.Content>
                  <Select.Item value="all">All</Select.Item>
                  <Select.Item value="debug">Debug</Select.Item>
                  <Select.Item value="info">Info</Select.Item>
                  <Select.Item value="warn">Warn</Select.Item>
                  <Select.Item value="error">Error</Select.Item>
                </Select.Content>
              </Select.Root>
              <Button variant="ghost" color="red" size="1" onClick={() => debugLog.clear()}>
                <TrashIcon width={12} height={12} />
              </Button>
              <Button
                variant="ghost"
                size="1"
                onClick={() => {
                  autoScrollRef.current = true;
                  if (scrollRef.current) {
                    scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
                  }
                }}
              >
                Bottom
              </Button>
            </>
          )}
          <IconButton variant="ghost" size="1" onClick={onClose}>
            <Cross2Icon />
          </IconButton>
        </Flex>
      </Flex>

      {expanded && (
        <div
          ref={scrollRef}
          onScroll={handleScroll}
          style={{ height: 220, overflowY: "auto", padding: "0 12px 8px", WebkitOverflowScrolling: "touch" }}
        >
          {filtered.length === 0 ? (
            <Flex align="center" justify="center" py="4">
              <Text size="1" color="gray">No log entries</Text>
            </Flex>
          ) : (
            filtered.map((entry, i) => <LogLine key={i} entry={entry} />)
          )}
        </div>
      )}
    </div>
  );
}
