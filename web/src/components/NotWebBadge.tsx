import { Badge } from "@radix-ui/themes";

/** Compact indicator for releases the browser cannot play without
 *  server transcoding: "WEB" struck through. Shown as the last icon in
 *  a variant row. */
export function NotWebBadge() {
  return (
    <Badge
      size="1"
      variant="soft"
      color="gray"
      style={{ flexShrink: 0, textDecoration: "line-through", opacity: 0.8 }}
      title="Served as-is (no server transcode); playback depends on your browser's codec support"
    >
      WEB
    </Badge>
  );
}
