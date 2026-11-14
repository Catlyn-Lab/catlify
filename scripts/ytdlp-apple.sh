#!/usr/bin/env bash
# Stages yt-dlp for the macOS app: builds `ytdlp-apple` (github.com/Catlyn-Lab/ytdlp-apple: yt-dlp on an
# embedded Python, same arguments and output as yt-dlp) and puts it, Python.framework and its Python
# packages where tauri.macos.conf.json bundles them. Catlify then needs no `brew install yt-dlp`.
# Run once (and after bumping VERSION) before `pnpm tauri build` on a Mac.
set -euo pipefail
VERSION="0.1.1"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PKG="$ROOT/third_party/ytdlp-apple"
TAURI="$ROOT/src-tauri"
TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"

if [ ! -d "$PKG/.git" ] || [ "$(git -C "$PKG" describe --tags 2>/dev/null)" != "$VERSION" ]; then
  rm -rf "$PKG"
  git clone --quiet --depth 1 --branch "$VERSION" https://github.com/Catlyn-Lab/ytdlp-apple.git "$PKG"
fi
swift build --package-path "$PKG" -c release --product ytdlp-apple
BIN_DIR="$(swift build --package-path "$PKG" -c release --show-bin-path)"

mkdir -p "$TAURI/binaries" "$TAURI/frameworks" "$TAURI/resources"
# The Python.framework SwiftPM downloaded for the macOS slice.
FW="$(find "$PKG/.build/artifacts" -path '*macos-arm64_x86_64/Python.framework' -maxdepth 6 -type d | head -1)"
[ -n "$FW" ] || { echo "Python.framework not found under $PKG/.build/artifacts" >&2; exit 1; }
rm -rf "$TAURI/frameworks/Python.framework" "$TAURI/resources/ytdlp-apple_YTDLP.bundle"
ditto "$FW" "$TAURI/frameworks/Python.framework"
ditto "$BIN_DIR/ytdlp-apple_YTDLP.bundle" "$TAURI/resources/ytdlp-apple_YTDLP.bundle"
cp "$BIN_DIR/ytdlp-apple" "$TAURI/binaries/ytdlp-apple-$TRIPLE"
# Inside Catlify.app it sits in Contents/MacOS, the framework in Contents/Frameworks.
install_name_tool -add_rpath @executable_path/../Frameworks "$TAURI/binaries/ytdlp-apple-$TRIPLE" 2>/dev/null || true
echo "staged ytdlp-apple $VERSION for $TRIPLE:"
du -sh "$TAURI/binaries/ytdlp-apple-$TRIPLE" "$TAURI/frameworks/Python.framework" "$TAURI/resources/ytdlp-apple_YTDLP.bundle"
