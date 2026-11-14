#!/usr/bin/env bash
# Minimal, audio-only, LGPL FFmpeg static libraries for Catlify.
#   scripts/build-ffmpeg.sh <target>
#   targets: macos-arm64 macos-x86_64 ios-arm64 ios-sim-arm64 android-arm64 android-x86_64
# Output: third_party/ffmpeg/<platform>/{include,lib/<arch>}
set -euo pipefail

VERSION=${FFMPEG_VERSION:-9.0.2}
TARGET=$1
ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK=${FFMPEG_WORK:-$ROOT/third_party/.ffmpeg-build}
SRC=$WORK/ffmpeg-$VERSION
JOBS=$(sysctl -n hw.ncpu 2>/dev/null || nproc)

mkdir -p "$WORK"
if [ ! -f "$SRC/.extracted" ]; then
  TARBALL=$WORK/ffmpeg-$VERSION.tar.xz
  until xz -t "$TARBALL" 2>/dev/null; do
    curl -sSL --retry 8 --retry-all-errors -C - -o "$TARBALL" "https://ffmpeg.org/releases/ffmpeg-$VERSION.tar.xz" || sleep 3
  done
  rm -rf "$SRC"
  tar -xJf "$TARBALL" -C "$WORK"
  touch "$SRC/.extracted"
fi

DEMUXERS=aac,aiff,ape,asf,caf,dsf,flac,iff,matroska,mov,mp3,mpc,mpc8,ogg,tak,tta,w64,wav,wv
DECODERS=aac,aac_latm,alac,ape,dsd_lsbf,dsd_lsbf_planar,dsd_msbf,dsd_msbf_planar,flac,mlp,mp3,mp3float,mpc7,mpc8,opus,tak,truehd,tta,vorbis,wavpack,wmalossless,wmapro,wmav1,wmav2
DECODERS=$DECODERS,pcm_alaw,pcm_f32be,pcm_f32le,pcm_f64be,pcm_f64le,pcm_mulaw,pcm_s16be,pcm_s16le,pcm_s24be,pcm_s24le,pcm_s32be,pcm_s32le,pcm_s8,pcm_u8
DECODERS=$DECODERS,mjpeg,png # embedded cover art streams
PARSERS=aac,aac_latm,flac,mlp,mpegaudio,opus,tak,vorbis

COMMON=(
  --disable-everything --disable-programs --disable-doc --disable-debug --disable-autodetect
  --disable-avdevice --disable-avfilter --disable-swscale --disable-network
  --enable-avformat --enable-avcodec --enable-swresample --enable-protocol=file,pipe
  --enable-demuxer=$DEMUXERS --enable-decoder=$DECODERS --enable-parser=$PARSERS
  --enable-static --disable-shared --enable-pic --enable-small --enable-zlib
)

case $TARGET in
  macos-arm64|macos-x86_64)
    ARCH=${TARGET#macos-}; PLATFORM=macos
    EXTRA=(--arch=$ARCH --cc="clang -arch $ARCH" --extra-cflags="-mmacosx-version-min=11.0" --extra-ldflags="-mmacosx-version-min=11.0")
    [ "$ARCH" != "$(uname -m)" ] && EXTRA+=(--enable-cross-compile --target-os=darwin)
    ;;
  ios-arm64)
    ARCH=arm64; PLATFORM=ios
    SDK=$(xcrun -sdk iphoneos --show-sdk-path)
    EXTRA=(--enable-cross-compile --target-os=darwin --arch=arm64 --cc="xcrun -sdk iphoneos clang"
      --sysroot="$SDK" --extra-cflags="-arch arm64 -miphoneos-version-min=14.0" --extra-ldflags="-arch arm64 -miphoneos-version-min=14.0")
    ;;
  ios-sim-arm64)
    ARCH=arm64; PLATFORM=ios-sim
    SDK=$(xcrun -sdk iphonesimulator --show-sdk-path)
    EXTRA=(--enable-cross-compile --target-os=darwin --arch=arm64 --cc="xcrun -sdk iphonesimulator clang"
      --sysroot="$SDK" --extra-cflags="-target arm64-apple-ios14.0-simulator" --extra-ldflags="-target arm64-apple-ios14.0-simulator")
    ;;
  android-arm64|android-x86_64)
    NDK=${ANDROID_NDK_HOME:-$(ls -d "$HOME"/Library/Android/sdk/ndk/* | sort -V | tail -1)}
    TC=$NDK/toolchains/llvm/prebuilt/$(ls "$NDK/toolchains/llvm/prebuilt" | head -1)
    API=24; PLATFORM=android
    if [ "$TARGET" = android-arm64 ]; then ARCH=arm64; FARCH=aarch64; TRIPLE=aarch64-linux-android; else ARCH=amd64; FARCH=x86_64; TRIPLE=x86_64-linux-android; fi
    EXTRA=(--enable-cross-compile --target-os=android --arch=$FARCH --cc="$TC/bin/$TRIPLE$API-clang"
      --cxx="$TC/bin/$TRIPLE$API-clang++" --ar="$TC/bin/llvm-ar" --nm="$TC/bin/llvm-nm" --ranlib="$TC/bin/llvm-ranlib"
      --strip="$TC/bin/llvm-strip" --sysroot="$TC/sysroot")
    [ "$FARCH" = x86_64 ] && EXTRA+=(--disable-asm) # NDK x86_64 + nasm isn't worth the setup for an emulator build
    ;;
  *) echo "unknown target $TARGET" >&2; exit 1 ;;
esac

OUT=$ROOT/third_party/ffmpeg/$PLATFORM
BUILD=$WORK/build-$TARGET
rm -rf "$BUILD"; mkdir -p "$BUILD"
cd "$BUILD"
"$SRC/configure" --prefix="$BUILD/prefix" "${COMMON[@]}" "${EXTRA[@]}" > configure.log 2>&1 || { tail -30 configure.log; tail -40 ffbuild/config.log; exit 1; }
make -j"$JOBS" > make.log 2>&1 || { tail -40 make.log; exit 1; }
make install > /dev/null

mkdir -p "$OUT/lib/$ARCH"
rm -rf "$OUT/include"; cp -R "$BUILD/prefix/include" "$OUT/include"
cp "$BUILD"/prefix/lib/*.a "$OUT/lib/$ARCH/"
# ffmpeg-sys-next falls back to lib/ when no arch dir matches.
[ -e "$OUT/lib/libavcodec.a" ] || cp "$BUILD"/prefix/lib/*.a "$OUT/lib/"
du -ch "$OUT/lib/$ARCH"/*.a | tail -1 | sed "s/total/total ($TARGET)/"
