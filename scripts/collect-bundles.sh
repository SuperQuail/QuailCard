#!/usr/bin/env bash
# 收集 Tauri 安装包到统一目录，供 GitHub Release 资产上传使用。
# 兼容两种产物布局：workspace 根 target/ 与历史 src-tauri/target/；
# 同名文件直接失败，避免不同架构的安装包互相覆盖。
set -euo pipefail

out="${1:-dist}"
rm -rf "$out"
mkdir -p "$out"
shopt -s nullglob

patterns=(
  "target/release/bundle/msi/*.msi"
  "target/release/bundle/nsis/*.exe"
  "target/release/bundle/dmg/*.dmg"
  "target/release/bundle/deb/*.deb"
  "target/release/bundle/rpm/*.rpm"
  "target/release/bundle/appimage/*.AppImage"
  "target/*/release/bundle/msi/*.msi"
  "target/*/release/bundle/nsis/*.exe"
  "target/*/release/bundle/dmg/*.dmg"
  "src-tauri/target/release/bundle/msi/*.msi"
  "src-tauri/target/release/bundle/nsis/*.exe"
  "src-tauri/target/release/bundle/dmg/*.dmg"
  "src-tauri/target/*/release/bundle/msi/*.msi"
  "src-tauri/target/*/release/bundle/nsis/*.exe"
  "src-tauri/target/*/release/bundle/dmg/*.dmg"
)

copied=0
for pattern in "${patterns[@]}"; do
  for file in $pattern; do
    name="$(basename "$file")"
    if [ -e "$out/$name" ]; then
      echo "错误：安装包重名 $name" >&2
      exit 1
    fi
    cp "$file" "$out/$name"
    copied=$((copied + 1))
  done
done

if [ "$copied" -eq 0 ]; then
  echo "错误：没有找到任何安装包" >&2
  exit 1
fi

echo "已收集 $copied 个安装包到 $out"
ls -1 "$out"
