#!/usr/bin/env bash
# 下载并解压 ECDICT 完整词典到构建资源目录（已存在则跳过）。
set -euo pipefail

URL="${1:-https://github.com/skywind3000/ECDICT/releases/download/1.0.28/ecdict-sqlite-28.zip}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="$ROOT/src-tauri/resources/ecdict.db"
ZIP="/tmp/ecdict-sqlite-28.zip"
EXTRACT="/tmp/ecdict-sqlite-28"

if [ -f "$TARGET" ]; then
  echo "ECDICT 词典已存在，跳过下载: $TARGET"
  exit 0
fi

echo "下载 ECDICT: $URL"
curl -L --fail --retry 2 -o "$ZIP" "$URL"

rm -rf "$EXTRACT"
mkdir -p "$EXTRACT"
unzip -q "$ZIP" -d "$EXTRACT"
mkdir -p "$(dirname "$TARGET")"
cp "$EXTRACT/stardict.db" "$TARGET"
echo "ECDICT 词典已就绪: $TARGET"
