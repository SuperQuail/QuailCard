# 下载并解压 ECDICT 完整词典到构建资源目录（已存在则跳过）。
param(
    [string]$Url = "https://github.com/skywind3000/ECDICT/releases/download/1.0.28/ecdict-sqlite-28.zip"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$target = Join-Path $root "src-tauri/resources/ecdict.db"
$zip = Join-Path $env:TEMP "ecdict-sqlite-28.zip"
$extract = Join-Path $env:TEMP "ecdict-sqlite-28"

if (Test-Path -LiteralPath $target) {
    Write-Host "ECDICT 词典已存在，跳过下载: $target"
    exit 0
}

Write-Host "下载 ECDICT: $Url"
curl.exe -L --fail --retry 2 -o $zip $Url
if ($LASTEXITCODE -ne 0) {
    throw "下载 ECDICT 失败"
}

if (Test-Path -LiteralPath $extract) {
    Remove-Item -LiteralPath $extract -Recurse -Force
}
Expand-Archive -LiteralPath $zip -DestinationPath $extract -Force
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
Copy-Item -LiteralPath (Join-Path $extract "stardict.db") -Destination $target
Write-Host "ECDICT 词典已就绪: $target"
