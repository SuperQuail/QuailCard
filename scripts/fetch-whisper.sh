#!/usr/bin/env bash
# 固定源码与目标架构；静态链接 whisper/ggml，避免宿主 CPU 指令或随包动态库泄漏。
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
version=v1.7.6
revision=a8d002cfd879315632a579e73f0148d06959de36
os="$(uname -s)"
case "${TARGET_ARCH:-$(uname -m)}" in
  x86_64|amd64|x64) arch=x86_64 ;;
  aarch64|arm64) arch=arm64 ;;
  *) echo "Unsupported TARGET_ARCH" >&2; exit 1 ;;
esac
case "$os" in
  Darwin) ;;
  Linux)
    host="$(uname -m)"; [ "$host" != aarch64 ] || host=arm64
    [ "$host" = "$arch" ] || { echo "Linux requires a native target runner" >&2; exit 1; } ;;
  *) echo "Use fetch-whisper.ps1 on Windows" >&2; exit 1 ;;
esac
metal=OFF
[ "$os-$arch" != Darwin-arm64 ] || metal=ON
flavor="static-baseline-metal-$metal-v2"
output="$root/src-tauri/resources/whisper"
identity="$os-$arch-$version-$flavor-$revision"
if [ -x "$output/whisper-cli" ] && [ -s "$output/LICENSE" ] &&
   [ -f "$output/build-id.txt" ] && [ "$(<"$output/build-id.txt")" = "$identity" ]; then
  echo "whisper-cli cache verified: $identity"; exit 0
fi
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
source="$work/source"
git init -q "$source"
git -C "$source" config core.autocrlf false
git -C "$source" remote add origin https://github.com/ggml-org/whisper.cpp.git
git -C "$source" -c http.version=HTTP/1.1 fetch --depth 1 origin "$revision"
git -C "$source" checkout -q --detach FETCH_HEAD
[ "$(git -C "$source" rev-parse HEAD)" = "$revision" ] || { echo "whisper revision mismatch" >&2; exit 1; }
args=(-DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DGGML_STATIC=OFF -DGGML_BACKEND_DL=OFF
  -DWHISPER_BUILD_TESTS=OFF -DWHISPER_BUILD_EXAMPLES=ON -DWHISPER_CURL=OFF
  -DGGML_NATIVE=OFF -DGGML_CUDA=OFF -DGGML_VULKAN=OFF -DGGML_BLAS=OFF -DGGML_ACCELERATE=OFF
  -DGGML_OPENMP=OFF -DGGML_METAL="$metal" -DGGML_METAL_EMBED_LIBRARY=ON
  -DGGML_SSE42=OFF -DGGML_BMI2=OFF -DGGML_AVX=OFF -DGGML_AVX2=OFF -DGGML_AVX_VNNI=OFF -DGGML_FMA=OFF
  -DGGML_F16C=OFF -DGGML_AVX512=OFF -DGGML_AVX512_VBMI=OFF -DGGML_AVX512_VNNI=OFF)
if [ "$os" = Darwin ]; then
  # Intel 包即便在 ARM runner 构建，也必须显式传递 SDK 架构。
  args+=(-DCMAKE_OSX_ARCHITECTURES="$arch" -DCMAKE_OSX_DEPLOYMENT_TARGET=11.0)
else
  args+=(-DCMAKE_EXE_LINKER_FLAGS="-static-libstdc++ -static-libgcc")
  [ "$arch" != arm64 ] || args+=(-DGGML_CPU_ARM_ARCH=armv8-a)
fi
cmake -S "$source" -B "$work/build" "${args[@]}"
cmake --build "$work/build" --config Release --target whisper-cli --parallel "${BUILD_JOBS:-2}"
binary="$work/build/bin/whisper-cli"
[ -x "$binary" ] || { echo "Missing whisper-cli build output" >&2; exit 1; }
if [ "$os" = Darwin ]; then
  lipo "$binary" -verify_arch "$arch"
  codesign --force --sign - "$binary"
else
  # 系统 libc 可动态链接，但不可依赖构建目录内的 whisper/ggml 或 OpenMP 库。
  if ldd "$binary" | grep -E 'lib(whisper|ggml|gomp)|not found'; then exit 1; fi
fi
if [ "$os" != Darwin ] || [ "$(uname -m)" = "$arch" ]; then
  "$binary" --help >/dev/null
fi
mkdir -p "$work/stage"
cp "$binary" "$work/stage/whisper-cli"
cp "$source/LICENSE" "$work/stage/LICENSE"
printf '%s\n' "$identity" > "$work/stage/build-id.txt"
printf 'Source: https://github.com/ggml-org/whisper.cpp/tree/%s\nVersion: %s\nFlavor: %s\n' "$revision" "$version" "$flavor" > "$work/stage/BUILD-INFO.txt"
mkdir -p "$(dirname "$output")"
rm -rf "$output"
mv "$work/stage" "$output"
echo "whisper-cli ready: $identity"
