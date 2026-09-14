# 固定 BtbN 官方 LGPL3 完整编解码构建；验证归档与程序摘要，不从滚动 latest 下载。
param(
    [string]$TargetArch = $env:TARGET_ARCH,
    [string]$Url,
    [string]$ArchivePath
)
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
if (-not $TargetArch) { $TargetArch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString() }
if ($TargetArch -notin @("x64", "amd64", "x86_64")) { throw "FFmpeg Windows bundle supports x86_64 only, not $TargetArch" }
$tag = "autobuild-2026-09-11-13-20"
$asset = "ffmpeg-N-126497-g5b614efc7e-win64-lgpl.zip"
$archiveSha = "17e902eed2544bd8d75982c69e091cbc9628c19a0fb8c86ca28926dd8f5215e4"
$binarySha = "d23eb7e3a456d321d6900d2a966d77bba536e06b40b1ac811da06cd16cbd3adc"
$ffmpegRevision = "5b614efc7e6134274fa5d05e240736be2dc203cc"
$recipesRevision = "cc8f0958be119db774cdaf6c50065651a4901e72"
$upstreamUrl = "https://github.com/BtbN/FFmpeg-Builds/releases/download/$tag/$asset"
if (-not $Url) { $Url = $upstreamUrl }
$downloadUri = [uri]$Url
if (-not $downloadUri.IsAbsoluteUri -or $downloadUri.Scheme -ne "https") { throw "FFmpeg mirror URL must use HTTPS" }
$root = Split-Path -Parent $PSScriptRoot
$output = Join-Path $root "src-tauri/resources/ffmpeg"
$target = Join-Path $output "ffmpeg.exe"
$identity = "Windows-x86_64-$tag-lgpl3-full-av1-v4-$archiveSha"
$complete = Test-Path (Join-Path $output "build-id.txt")
foreach ($required in @("ffmpeg.exe", "LICENSE.txt", "COPYING.GPLv3", "SOURCE-INFO.txt", "BUILD-INFO.txt", "ffmpeg-source.tar.gz", "btbn-build-recipes.tar.gz")) {
    $complete = $complete -and (Test-Path (Join-Path $output $required))
}
if ($complete -and (Get-Content (Join-Path $output "build-id.txt") -Raw).Trim() -eq $identity -and
    (Get-FileHash $target -Algorithm SHA256).Hash.ToLowerInvariant() -eq $binarySha) {
    Write-Host "FFmpeg cache verified: $identity"
    return
}
$work = Join-Path ([IO.Path]::GetTempPath()) ("quailcard-ffmpeg-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $work | Out-Null
# 原生命令非零状态必须转成异常；PowerShell 默认不会因 git/ffmpeg 失败而停止。
function Invoke-Checked([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed (exit $LASTEXITCODE)" }
}
try {
    if (-not $ArchivePath) {
        $ArchivePath = Join-Path $work "package.zip"
        Invoke-Checked "curl.exe" @("--proto", "=https", "--tlsv1.2", "-fL", "--retry", "3", "--connect-timeout", "30", "-o", $ArchivePath, $url)
    }
    if ((Get-FileHash $ArchivePath -Algorithm SHA256).Hash.ToLowerInvariant() -ne $archiveSha) { throw "FFmpeg archive SHA256 mismatch" }
    Expand-Archive -LiteralPath $ArchivePath -DestinationPath (Join-Path $work "package")
    $package = Join-Path $work "package/ffmpeg-N-126497-g5b614efc7e-win64-lgpl"
    $binary = Join-Path $package "bin/ffmpeg.exe"
    if ((Get-FileHash $binary -Algorithm SHA256).Hash.ToLowerInvariant() -ne $binarySha) { throw "FFmpeg executable SHA256 mismatch" }
    $configuration = (& $binary -version 2>&1 | Out-String)
    if ($LASTEXITCODE -ne 0 -or $configuration -match '--enable-(gpl|nonfree)(\s|$)') { throw "Unexpected FFmpeg build or license" }
    $license = (& $binary -L 2>&1 | Out-String)
    if ($LASTEXITCODE -ne 0 -or $license -notmatch 'GNU Lesser General Public' -or $license -notmatch 'version 3') { throw "Expected LGPL-3.0-or-later FFmpeg" }
    # 实际解码 AV1 后缩放输出 JPEG，验证软件解码与现有抽帧契约，而不依赖 GPU 驱动。
    $fixture = Join-Path $work "av1.ivf"
    [IO.File]::WriteAllBytes($fixture, [Convert]::FromBase64String("REtJRgAAIABBVjAxEAAQAAEAAAABAAAAAQAAAAAAAAAeAAAAAAAAAAAAAAASAAoKAAAAAZ/5tfIAgDIOEADAAAACgAAAAKmOWNQ="))
    Invoke-Checked $binary @("-nostdin", "-hide_banner", "-loglevel", "error", "-c:v", "libdav1d", "-i", $fixture, "-frames:v", "1", "-vf", "scale=8:-2", "-q:v", "2", (Join-Path $work "frame.jpg"))
    if (-not (Test-Path (Join-Path $work "frame.jpg"))) { throw "FFmpeg AV1 frame smoke failed" }
    $stage = Join-Path $work "stage"
    New-Item -ItemType Directory -Path $stage | Out-Null
    Copy-Item (Join-Path $package "LICENSE.txt") $stage
    Copy-Item (Join-Path $package "doc") (Join-Path $stage "upstream-doc") -Recurse
    # 包含精确 FFmpeg 源码与 BtbN 构建配方；第三方对应源码发布义务在 SOURCE-INFO 中明确说明。
    foreach ($source in @(
        @{ Name = "ffmpeg-source"; Url = "https://github.com/FFmpeg/FFmpeg.git"; Revision = $ffmpegRevision },
        @{ Name = "btbn-build-recipes"; Url = "https://github.com/BtbN/FFmpeg-Builds.git"; Revision = $recipesRevision }
    )) {
        $directory = Join-Path $work $source.Name
        Invoke-Checked "git" @("init", "-q", $directory)
        Invoke-Checked "git" @("-C", $directory, "config", "core.autocrlf", "false")
        Invoke-Checked "git" @("-C", $directory, "remote", "add", "origin", $source.Url)
        Invoke-Checked "git" @("-C", $directory, "-c", "http.version=HTTP/1.1", "fetch", "--depth", "1", "origin", $source.Revision)
        Invoke-Checked "git" @("-C", $directory, "checkout", "-q", "--detach", "FETCH_HEAD")
        $actual = (& git -C $directory rev-parse HEAD | Out-String).Trim()
        if ($LASTEXITCODE -ne 0 -or $actual -ne $source.Revision) { throw "Source revision mismatch" }
        Invoke-Checked "git" @("-C", $directory, "archive", "--format=tar.gz", "--prefix=$($source.Name)/", "--output=$(Join-Path $stage ($source.Name + '.tar.gz'))", "HEAD")
    }
    Copy-Item (Join-Path $work "ffmpeg-source/COPYING.GPLv3") $stage
    @"
FFmpeg N-126497-g5b614efc7e-20260911; Windows x86_64; LGPL-3.0-or-later (NOT GPL, NOT LGPL2.1).
Canonical binary distribution: $upstreamUrl
Mirror/archive overrides never change the required SHA256.
Retention: upstream dated daily assets may be pruned. Retain this verified ZIP in your artifact
store; CI can pass -Url https://your-mirror/... or -ArchivePath to the exact same archive.
Mirror URLs (which may contain credentials) are intentionally not persisted in this manifest.
Archive SHA256: $archiveSha
Executable SHA256: $binarySha
FFmpeg source: https://github.com/FFmpeg/FFmpeg/tree/$ffmpegRevision
BtbN build recipes: https://github.com/BtbN/FFmpeg-Builds/tree/$recipesRevision
Complete FFmpeg source and the matching BtbN build recipes are bundled in the two tar.gz files.
The executable is a separate replaceable process; users may rebuild/replace it.
Redistribution notice: these archives are NOT the complete corresponding source of every statically
linked third-party library. Before publishing, retain/mirror matching dependency sources and notices
identified by the BtbN recipes and upstream-doc/general.html, and provide the legally required source
access alongside the binary distribution. Do not describe the recipe archive as all dependency sources.

$configuration
"@ | Set-Content (Join-Path $stage "SOURCE-INFO.txt") -Encoding utf8
    Copy-Item (Join-Path $stage "SOURCE-INFO.txt") (Join-Path $stage "BUILD-INFO.txt")
    New-Item -ItemType Directory -Force -Path $output | Out-Null
    # 已有的经过精确摘要验证的程序保持不动，仅补齐许可和来源材料。
    if (-not (Test-Path $target) -or (Get-FileHash $target -Algorithm SHA256).Hash.ToLowerInvariant() -ne $binarySha) {
        Copy-Item $binary $target -Force
    }
    Get-ChildItem $stage | Copy-Item -Destination $output -Recurse -Force
    $identity | Set-Content (Join-Path $output "build-id.txt") -Encoding utf8
    Write-Host "FFmpeg full-codec bundle ready: $identity"
} finally {
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}
