#!/usr/bin/env bash
# Unix 从固定提交构建 LGPL 音视频提取器与软件 AV1；Windows 使用单独的校验预编译脚本。
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
version=7.1.1
revision=db69d06eeeab4f46da15030a80d539efb4503ca8
aom_version=3.12.1
aom_revision=10aece4157eb79315da205f39e19bf6ab3ee30d0
flavor=lgpl-media-av1-static-v4
os="$(uname -s)"
case "${TARGET_ARCH:-$(uname -m)}" in
  x86_64|amd64|x64) arch=x86_64 ;;
  aarch64|arm64) arch=arm64 ;;
  *) echo "Unsupported TARGET_ARCH" >&2; exit 1 ;;
esac
exe=ffmpeg
platform=()
case "$os" in
  Darwin)
    platform=(--target-os=darwin --cc=clang --arch="$arch"
      --extra-cflags="-arch $arch -mmacosx-version-min=11.0"
      --extra-ldflags="-arch $arch -mmacosx-version-min=11.0")
    # ARM runner 生成 Intel Mach-O；configure 不得执行目标探针。
    [ "$(uname -m)" = "$arch" ] || platform+=(--enable-cross-compile) ;;
  Linux)
    host="$(uname -m)"; [ "$host" != aarch64 ] || host=arm64
    [ "$host" = "$arch" ] || { echo "Linux requires a native target runner" >&2; exit 1; }
    platform=(--arch="$arch" --extra-ldflags=-static-libgcc) ;;
  MINGW*|MSYS*) echo "Use fetch-ffmpeg.ps1 for verified full-codec Windows bundle" >&2; exit 1 ;;
  *) echo "Unsupported platform: $os" >&2; exit 1 ;;
esac
output="$root/src-tauri/resources/ffmpeg"
identity="$os-$arch-$version-$revision-aom-$aom_revision-$flavor"
if [ -x "$output/$exe" ] && [ -s "$output/COPYING.LGPLv2.1" ] &&
   [ -s "$output/ffmpeg-source.tar.gz" ] && [ -s "$output/aom-source.tar.gz" ] &&
   [ -f "$output/build-id.txt" ] &&
   [ "$(<"$output/build-id.txt")" = "$identity" ]; then
  echo "FFmpeg cache verified: $identity"; exit 0
fi
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
# 固定提交下载可复用于 FFmpeg 与 AV1 库；不使用随时间变化的分支头。
fetch_source() {
  local directory="$1" url="$2" commit="$3"
  git init -q "$directory"
  git -C "$directory" config core.autocrlf false
  git -C "$directory" remote add origin "$url"
  git -C "$directory" -c http.version=HTTP/1.1 fetch --depth 1 origin "$commit"
  git -C "$directory" checkout -q --detach FETCH_HEAD
  [ "$(git -C "$directory" rev-parse HEAD)" = "$commit" ] || { echo "Source revision mismatch" >&2; exit 1; }
}
source="$work/source"
aom="$work/aom"
fetch_source "$source" https://github.com/FFmpeg/FFmpeg.git "$revision"
fetch_source "$aom" https://aomedia.googlesource.com/aom "$aom_revision"
# 原生 av1 解码器依赖硬件；静态 libaom 提供所有平台可用的软件 AV1，不降级用户视频选择。
aom_args=(-DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$work/deps" -DCMAKE_INSTALL_LIBDIR=lib
  -DBUILD_SHARED_LIBS=OFF -DCMAKE_POSITION_INDEPENDENT_CODE=ON -DAOM_TARGET_CPU=generic
  -DCONFIG_AV1_ENCODER=0 -DCONFIG_AV1_DECODER=1 -DENABLE_TESTS=OFF -DENABLE_DOCS=OFF
  -DENABLE_EXAMPLES=OFF -DENABLE_TOOLS=OFF)
if [ "$os" = Darwin ]; then
  aom_args+=(-DCMAKE_OSX_ARCHITECTURES="$arch" -DCMAKE_OSX_DEPLOYMENT_TARGET=11.0)
fi
cmake -S "$aom" -B "$work/aom-build" "${aom_args[@]}"
cmake --build "$work/aom-build" --target install --parallel "${BUILD_JOBS:-2}"
export PKG_CONFIG_PATH="$work/deps/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
pkg-config --exists aom
# HTTP/TCP 仅供 Rust 的回环 Range 代理；本地调用方仍使用 file 协议白名单。
# PNG/APNG（需 zlib）不在此视频处理构建内；AV1 使用显式启用的静态 libaom。
args=(--disable-gpl --disable-nonfree --disable-version3 --disable-autodetect
  --disable-everything --enable-network --disable-doc --disable-debug --disable-asm
  --disable-shared --enable-static --disable-programs --enable-ffmpeg
  --enable-libaom --pkg-config-flags=--static
  --disable-avdevice --disable-postproc --enable-swscale
  --enable-protocol=file,pipe,http,tcp
  --enable-demuxer=ivf,mov,matroska,avi,flv,mpegts,mpegps,wav,mp3,ogg,aac,flac,asf,image2,mjpeg
  --enable-decoder=libaom_av1,h264,hevc,vp8,vp9,mpeg4,mpeg1video,mpeg2video,mjpeg,ppm,aac,aac_latm,mp3,mp3float,opus,vorbis,flac,alac,ac3,eac3,wmav1,wmav2,pcm_s16le,pcm_s24le,pcm_s32le,pcm_f32le,pcm_f64le,pcm_u8,pcm_s16be
  --enable-parser=av1,h264,hevc,vp8,vp9,mpeg4video,mpegvideo,mjpeg,pnm,aac,aac_latm,mpegaudio,opus,vorbis,flac,ac3
  --enable-encoder=pcm_s16le,mjpeg --enable-muxer=wav,image2
  --enable-filter=scale,format,null,buffer,buffersink,trim,setpts,aresample,aformat,anull,abuffer,abuffersink,atrim,asetpts
  "${platform[@]}")
mkdir -p "$work/build" "$work/stage"
(
  cd "$work/build"
  "$source/configure" "${args[@]}"
  # configure 对缺失依赖可能只警告并关闭组件；跨架构也必须验证实际能力。
  for component in LIBAOM_AV1_DECODER H264_DECODER HEVC_DECODER VP8_DECODER VP9_DECODER MPEG4_DECODER \
    MJPEG_DECODER PPM_DECODER AAC_DECODER PCM_S16LE_DECODER MJPEG_ENCODER \
    PCM_S16LE_ENCODER WAV_MUXER IMAGE2_MUXER IVF_DEMUXER MOV_DEMUXER IMAGE2_DEMUXER \
    SCALE_FILTER ARESAMPLE_FILTER HTTP_PROTOCOL TCP_PROTOCOL; do
    grep -Fxq "#define CONFIG_$component 1" config_components.h || {
      echo "Required FFmpeg component disabled: $component" >&2; exit 1;
    }
  done
  make -j"${BUILD_JOBS:-2}" "$exe"
)
binary="$work/build/$exe"
[ -x "$binary" ] || { echo "Missing FFmpeg build output" >&2; exit 1; }
# 用 configure 的判定而非第三方文件名确认许可；禁止将 GPL 构建标注为 LGPL。
grep -Fxq '#define FFMPEG_LICENSE "LGPL version 2.1 or later"' "$work/build/config.h" || {
  echo "Unexpected FFmpeg license; refusing to bundle" >&2; exit 1;
}
if [ "$os" = Darwin ]; then
  lipo -verify_arch "$arch" "$binary"
  codesign --force --sign - "$binary"
else
  "$binary" -version
  "$binary" -L
fi
if [ "$os" = Linux ]; then
  if ldd "$binary" | grep -E 'libaom|libav(codec|format|util|filter)|libsw(resample|scale)|not found'; then exit 1; fi
else
  if otool -L "$binary" | grep -E 'libaom|libav(codec|format|util|filter)|libsw(resample|scale)'; then exit 1; fi
fi
if [ "$os" != Darwin ] || [ "$(uname -m)" = "$arch" ]; then
  # 用 8kHz WAV 验证真实解码、重采样和 16kHz PCM 输出；跨架构 Mac 不依赖 Rosetta。
  printf 'RIFF\xa4\x3e\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x40\x1f\0\0\x80\x3e\0\0\x02\0\x10\0data\x80\x3e\0\0' > "$work/input.wav"
  dd if=/dev/zero bs=16000 count=1 >> "$work/input.wav" 2>/dev/null
  "$binary" -nostdin -hide_banner -loglevel error -i "$work/input.wav" \
    -vn -ac 1 -ar 16000 -c:a pcm_s16le "$work/output.wav"
  [ -s "$work/output.wav" ] || { echo "FFmpeg audio smoke test failed" >&2; exit 1; }
  # PPM 是无需外部图片依赖的测试输入，验证 scale、像素转换、MJPEG 编码和 image2 输出。
  printf 'P6\n16 16\n255\n' > "$work/input.ppm"
  dd if=/dev/zero bs=768 count=1 >> "$work/input.ppm" 2>/dev/null
  "$binary" -nostdin -hide_banner -loglevel error -protocol_whitelist file \
    -i "$work/input.ppm" -frames:v 1 -vf "scale='min(8,iw)':-2" -q:v 2 "$work/frame.jpg"
  [ -s "$work/frame.jpg" ] || { echo "FFmpeg frame smoke test failed" >&2; exit 1; }
  # 随脚本携带自生成的 16x16 AV1 黑帧；强制软件解码后走同一 JPEG 输出路径。
  decode_flag=-d; [ "$os" != Darwin ] || decode_flag=-D
  printf '%s' 'REtJRgAAIABBVjAxEAAQAAEAAAABAAAAAQAAAAAAAAAeAAAAAAAAAAAAAAASAAoKAAAAAZ/5tfIAgDIOEADAAAACgAAAAKmOWNQ=' | base64 "$decode_flag" > "$work/av1.ivf"
  "$binary" -nostdin -hide_banner -loglevel error -c:v libaom-av1 -i "$work/av1.ivf" \
    -frames:v 1 -vf scale=8:-2 -q:v 2 "$work/av1.jpg"
  [ -s "$work/av1.jpg" ] || { echo "FFmpeg software AV1 smoke test failed" >&2; exit 1; }
  "$binary" -protocols > "$work/protocols.txt"
  grep -Eq '^[[:space:]]+http$' "$work/protocols.txt"
  grep -Eq '^[[:space:]]+tcp$' "$work/protocols.txt"
fi
cp "$binary" "$work/stage/$exe"
cp "$source/COPYING.LGPLv2.1" "$source/LICENSE.md" "$work/stage/"
cp "$aom/LICENSE" "$work/stage/AOM-LICENSE"
cp "$aom/PATENTS" "$work/stage/AOM-PATENTS"
git -C "$aom" archive --format=tar.gz --prefix="aom-$aom_version/" HEAD > "$work/stage/aom-source.tar.gz"
# 随包保留完整对应源码与精确配置，用户可修改、重建并替换独立的 FFmpeg 程序。
git -C "$source" archive --format=tar.gz --prefix="ffmpeg-$version/" HEAD > "$work/stage/ffmpeg-source.tar.gz"
{
  printf 'FFmpeg %s\nSource: https://github.com/FFmpeg/FFmpeg/tree/%s\nLicense: LGPL-2.1-or-later\nTarget: %s-%s\nFlavor: %s\n' "$version" "$revision" "$os" "$arch" "$flavor"
  printf 'AV1 source: https://aomedia.googlesource.com/aom/+/%s\nAV1 version: %s; BSD-2-Clause + patent grant.\n' "$aom_revision" "$aom_version"
  printf 'Build libaom first: extract aom-source.tar.gz and run cmake -S aom-%s -B aom-build ' "$aom_version"
  printf '%q ' "${aom_args[@]}"
  printf '\ncmake --build aom-build --target install\nexport PKG_CONFIG_PATH=%q\n' "$work/deps/lib/pkgconfig"
  printf 'The recorded dependency prefix may be replaced with a writable local prefix.\nBuild FFmpeg: extract ffmpeg-source.tar.gz, enter its directory, then run:\n./configure '
  printf '%q ' "${args[@]}"
  printf '\nmake -j2 %s\nStatically linked libaom supplies software AV1; OS system libraries otherwise.\nPNG/APNG image input is not included.\n' "$exe"
} > "$work/stage/BUILD-INFO.txt"
printf '%s\n' "$identity" > "$work/stage/build-id.txt"
mkdir -p "$(dirname "$output")"
rm -rf "$output"
mv "$work/stage" "$output"
echo "FFmpeg ready: $identity"
