#!/usr/bin/env bash
# Usage: scripts/android.sh dev|build [tauri android args...]
# Sets up SDK/NDK/JDK and the static FFmpeg so `tauri android` builds out of the box.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
export NDK_HOME="${NDK_HOME:-$(ls -d "$ANDROID_HOME"/ndk/* | sort -V | tail -1)}"
if [ -z "${JAVA_HOME:-}" ] && [ -d "/Applications/Android Studio.app/Contents/jbr/Contents/Home" ]; then
  export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
fi
export FFMPEG_DIR="$ROOT/third_party/ffmpeg/android"

# ffmpeg-sys-next runs bindgen; point clang at the NDK sysroot for each Android target.
SYSROOT="$(ls -d "$NDK_HOME"/toolchains/llvm/prebuilt/*/sysroot | head -1)"
export BINDGEN_EXTRA_CLANG_ARGS_aarch64_linux_android="--sysroot=$SYSROOT --target=aarch64-linux-android26"
export BINDGEN_EXTRA_CLANG_ARGS_x86_64_linux_android="--sysroot=$SYSROOT --target=x86_64-linux-android26"

cd "$ROOT"
exec pnpm tauri android "$@"
