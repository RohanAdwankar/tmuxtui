#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
image="tmuxtui-demo:local"

docker build -f "$root/demo/Dockerfile" -t "$image" "$root"
mkdir -p "$root/demo/out"
docker run --rm \
  -v "$root:/work" \
  -w /work \
  -e CARGO_HOME=/tmp/cargo \
  -e CARGO_TARGET_DIR=/tmp/tmuxtui-target \
  "$image" \
  uv run --no-project --script /work/demo/preview_scroll.py
