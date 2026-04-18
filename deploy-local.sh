#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BINARY="$SCRIPT_DIR/target/release/streamx"
PORT=8999
BIND="0.0.0.0"
WORKDIR="$SCRIPT_DIR/crates/server"

if [ ! -f "$BINARY" ]; then
  echo "Binary not found at $BINARY - run 'cargo build --release' first"
  exit 1
fi

# Gracefully stop existing instance
PIDS=$(pgrep -f "streamx.*--port $PORT" 2>/dev/null || true)
if [ -n "$PIDS" ]; then
  echo "Stopping existing StreamX (PIDs: $PIDS)..."
  kill -TERM $PIDS 2>/dev/null || true
  # Wait up to 5 seconds for graceful shutdown
  for i in $(seq 1 50); do
    if ! pgrep -f "streamx.*--port $PORT" >/dev/null 2>&1; then
      break
    fi
    sleep 0.1
  done
  # Force kill if still running
  PIDS=$(pgrep -f "streamx.*--port $PORT" 2>/dev/null || true)
  if [ -n "$PIDS" ]; then
    echo "Force killing (PIDs: $PIDS)..."
    kill -9 $PIDS 2>/dev/null || true
    sleep 0.5
  fi
  echo "Stopped."
fi

# Start new instance
echo "Starting StreamX on port $PORT..."
cd "$WORKDIR"
"$BINARY" --port "$PORT" --bind "$BIND" &disown

# Wait for it to be ready
for i in $(seq 1 20); do
  if curl -s -o /dev/null -w '' "http://localhost:$PORT/" 2>/dev/null; then
    echo "StreamX is running on http://localhost:$PORT"
    exit 0
  fi
  sleep 0.5
done

echo "WARNING: StreamX did not respond within 10 seconds"
exit 1
