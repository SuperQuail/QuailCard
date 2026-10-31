#!/usr/bin/env bash
# 依次准备 ffmpeg / whisper / ecdict；任一脚本失败立即退出，
# 避免串联执行时失败被后续脚本的退出码掩盖。
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
for script in fetch-ffmpeg.sh fetch-whisper.sh fetch-ecdict.sh; do
  echo "=== $script ==="
  bash "./scripts/$script"
done
echo "所有随包组件已就绪"
