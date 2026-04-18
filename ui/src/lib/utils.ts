export function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const k = 1024;
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  const unit = units[i] ?? "TB";
  return `${(bytes / Math.pow(k, i)).toFixed(1)} ${unit}`;
}

export function formatDuration(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);

  if (h > 0) {
    return `${h}:${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}`;
  }
  return `${m}:${s.toString().padStart(2, "0")}`;
}

export function formatSpeed(bytesPerSec: number): string {
  return `${formatBytes(bytesPerSec)}/s`;
}

export function classNames(
  ...classes: (string | undefined | null | false)[]
): string {
  return classes.filter(Boolean).join(" ");
}

export function detectQuality(title: string): string | null {
  const lower = title.toLowerCase();
  if (lower.includes("2160p") || lower.includes("4k")) return "4K";
  if (lower.includes("1080p")) return "1080p";
  if (lower.includes("720p")) return "720p";
  if (lower.includes("480p")) return "480p";
  return null;
}

export function isMagnetLink(text: string): boolean {
  return text.trim().startsWith("magnet:");
}
