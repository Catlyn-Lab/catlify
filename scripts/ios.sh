#!/usr/bin/env bash
# Usage: scripts/ios.sh dev|build [tauri ios args...]   (IOS_SIM=1 for simulator FFmpeg)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
if [ "${IOS_SIM:-0}" = 1 ]; then
  export FFMPEG_DIR="$ROOT/third_party/ffmpeg/ios-sim"
else
  export FFMPEG_DIR="$ROOT/third_party/ffmpeg/ios"
fi
cd "$ROOT"
exec pnpm tauri ios "$@"
