type LogLevel = "info" | "warn" | "error" | "debug";

interface LogEntry {
  timestamp: number;
  level: LogLevel;
  source: string;
  message: string;
  data?: unknown;
}

class DebugLogger {
  private entries: LogEntry[] = [];
  private listeners: Set<() => void> = new Set();
  private maxEntries = 500;

  log(level: LogLevel, source: string, message: string, data?: unknown) {
    const entry: LogEntry = {
      timestamp: Date.now(),
      level,
      source,
      message,
      data,
    };
    this.entries.push(entry);
    if (this.entries.length > this.maxEntries) {
      this.entries = this.entries.slice(-this.maxEntries);
    }
    const consoleFn =
      level === "error"
        ? console.error
        : level === "warn"
          ? console.warn
          : console.log;
    consoleFn(`[${source}] ${message}`, data ?? "");
    this.listeners.forEach((fn) => fn());
  }

  info(source: string, msg: string, data?: unknown) {
    this.log("info", source, msg, data);
  }
  warn(source: string, msg: string, data?: unknown) {
    this.log("warn", source, msg, data);
  }
  error(source: string, msg: string, data?: unknown) {
    this.log("error", source, msg, data);
  }
  debug(source: string, msg: string, data?: unknown) {
    this.log("debug", source, msg, data);
  }

  getEntries(): LogEntry[] {
    return [...this.entries];
  }

  clear() {
    this.entries = [];
    this.listeners.forEach((fn) => fn());
  }

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }
}

export const debugLog = new DebugLogger();
export type { LogEntry, LogLevel };
