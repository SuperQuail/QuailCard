# 依次准备 ffmpeg / whisper / ecdict；任一脚本失败立即终止并返回非零退出码。
# 之前三个脚本串在同一个 run 步骤里，第一个抛错后 PowerShell 仍会继续执行后面的脚本，
# 步骤整体退出码取自最后一个，导致失败被吞掉、最终在 tauri build 报“资源不存在”。
param()
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$steps = @("fetch-ffmpeg.ps1", "fetch-whisper.ps1", "fetch-ecdict.ps1")
foreach ($step in $steps) {
    $path = Join-Path $PSScriptRoot $step
    Write-Host "=== $step ==="
    & $path
    if ($LASTEXITCODE -ne 0) { throw "$step failed (exit $LASTEXITCODE)" }
}
Write-Host "所有随包组件已就绪"
