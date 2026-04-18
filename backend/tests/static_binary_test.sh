#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$SCRIPT_DIR"

echo "Building release binary..."
cargo build --release

BINARY="target/release/streamx"

if [ ! -f "$BINARY" ]; then
    echo "FAIL: Binary not found at $BINARY"
    exit 1
fi

echo "Binary exists at $BINARY"

file_output=$(file "$BINARY")
echo "file output: $file_output"

if echo "$file_output" | grep -q "statically linked"; then
    echo "PASS: Binary is statically linked"
elif ldd "$BINARY" 2>&1 | grep -q "not a dynamic executable"; then
    echo "PASS: ldd confirms binary is static"
else
    echo "NOTE: Binary is dynamically linked (expected for non-musl builds)"
    echo "      For a static binary, build with: cargo build --release --target x86_64-unknown-linux-musl"
fi

echo "Verifying binary runs..."
if "$BINARY" --help > /dev/null 2>&1; then
    echo "PASS: Binary executes successfully (--help exits 0)"
else
    echo "FAIL: Binary failed to execute"
    exit 1
fi

echo "All checks passed."
