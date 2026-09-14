# 固定源码构建 Windows Vulkan + 独立 CPU 回退；不下载 CUDA，不修改系统驱动。
param(
    [string]$TargetArch = $env:TARGET_ARCH,
    [string]$SourceUrl = "https://github.com/ggml-org/whisper.cpp.git",
    [string]$WorkDir = "",
    [switch]$Force
)
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
. "$PSScriptRoot/component-toolchain.ps1"
$root = Split-Path -Parent $PSScriptRoot
if (-not $TargetArch) { $TargetArch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString() }
if ($TargetArch -notin @("x64", "amd64", "x86_64")) { throw "Windows Vulkan 当前只交付 x86_64；不能用宿主架构冒充 $TargetArch" }
$revision = "a8d002cfd879315632a579e73f0148d06959de36"
$identity = "Windows-x86_64-whisper-v1.7.6-vulkan-cpu-sse2-staticcrt-v3-$revision"
$output = Join-Path $root "src-tauri/resources/whisper"
if (-not $WorkDir) { $WorkDir = Join-Path $root ".component-build" }
New-Item -ItemType Directory -Force $WorkDir | Out-Null
$WorkDir = (Resolve-Path $WorkDir).Path
$manifest = Join-Path $output "component.json"
if (-not $Force -and (Test-Path $manifest)) {
    try {
        $cached = Get-Content -Raw $manifest | ConvertFrom-Json
        $valid = $cached.identity -eq $identity
        foreach ($file in $cached.files) {
            $path = Join-Path $output $file.path
            $valid = $valid -and (Test-Path $path) -and ((Get-FileHash $path -Algorithm SHA256).Hash -eq $file.sha256)
        }
        if ($valid -and $cached.files.Count -ge 4) { Write-Host "whisper cache verified: $identity"; exit 0 }
    } catch { Write-Host "组件缓存不完整，重新构建" }
}
$cmake = Get-ComponentCmake $WorkDir
$env:VULKAN_SDK = Get-ComponentVulkan $WorkDir
$source = Join-Path $WorkDir "whisper-src"
if (-not (Test-Path "$source/.git")) {
    git clone --depth 1 --branch v1.7.6 $SourceUrl $source
    if ($LASTEXITCODE -ne 0) { throw "whisper 源码下载失败" }
}
$actual = git -C $source rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $actual -ne $revision) { throw "whisper 源码提交与固定版本不匹配" }
# 同时固定库和 CRT 为静态；旧 CMakeLists 需要显式启用 CMP0091 才会使用 /MT。
$common = @("-G", "Visual Studio 17 2022", "-A", "x64",
    "-DCMAKE_POLICY_DEFAULT_CMP0091=NEW", "-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded",
    "-DBUILD_SHARED_LIBS=OFF", "-DGGML_BACKEND_DL=OFF", "-DGGML_NATIVE=OFF",
    "-DGGML_SSE42=OFF", "-DGGML_BMI2=OFF", "-DGGML_AVX=OFF", "-DGGML_AVX2=OFF",
    "-DGGML_AVX_VNNI=OFF", "-DGGML_AVX512=OFF", "-DGGML_FMA=OFF", "-DGGML_F16C=OFF",
    "-DGGML_OPENMP=OFF", "-DGGML_CUDA=OFF", "-DGGML_BLAS=OFF",
    "-DWHISPER_BUILD_TESTS=OFF", "-DWHISPER_BUILD_EXAMPLES=ON", "-DWHISPER_CURL=OFF")
$stage = Join-Path $WorkDir ("whisper-stage-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force "$stage/cpu" | Out-Null
foreach ($flavor in @("cpu", "vulkan")) {
    $build = Join-Path $WorkDir "whisper-$flavor"
    $gpu = if ($flavor -eq "vulkan") { "ON" } else { "OFF" }
    & $cmake -S $source -B $build @common "-DGGML_VULKAN=$gpu"
    if ($LASTEXITCODE -ne 0) { throw "whisper $flavor 配置失败" }
    & $cmake --build $build --config Release --target whisper-cli --parallel 4
    if ($LASTEXITCODE -ne 0) { throw "whisper $flavor 构建失败" }
    $destination = if ($flavor -eq "cpu") { "$stage/cpu" } else { $stage }
    Copy-Item "$build/bin/Release/whisper-cli.exe" "$destination/whisper-cli.exe"
}
# Loader 是 Khronos 可再分发运行库，不包含厂商 ICD；真正设备仍需显卡驱动。
$runtimeZip = Join-Path $WorkDir "vulkan-runtime.zip"
Get-VerifiedFile "https://sdk.lunarg.com/sdk/download/1.4.321.0/windows/VulkanRT-X64-1.4.321.0-Components.zip" $runtimeZip "f8ba79303face4b72a2f302f1a46e8c918995b54fbf24c0c9fc6c3d3ef525dca"
$runtime = Join-Path $WorkDir "vulkan-runtime"
Expand-Archive $runtimeZip $runtime -Force
$loader = Get-ChildItem $runtime -Recurse -Filter vulkan-1.dll | Where-Object { $_.Directory.Name -eq "x64" } | Select-Object -First 1
if (-not $loader) { throw "未找到 x64 Vulkan loader" }
Copy-Item $loader.FullName "$stage/vulkan-1.dll"
Copy-Item "$source/LICENSE" "$stage/LICENSE"
$license = Get-ChildItem $runtime -Recurse -File | Where-Object Name -Match "LICENSE|COPYING" | Select-Object -First 1
if (-not $license) { throw "Vulkan loader 许可缺失，拒绝打包" }
Copy-Item $license.FullName "$stage/VULKAN-LICENSE.txt"
# 在搬离构建目录的干净目录中验证独立 CPU；GPU 没设备不是构建失败。
& "$stage/cpu/whisper-cli.exe" --help
if ($LASTEXITCODE -ne 0) { throw "CPU 独立运行自检失败" }
$files = @("whisper-cli.exe", "cpu/whisper-cli.exe", "vulkan-1.dll", "LICENSE", "VULKAN-LICENSE.txt") | ForEach-Object {
    @{ path = $_; sha256 = (Get-FileHash (Join-Path $stage $_) -Algorithm SHA256).Hash.ToLowerInvariant() }
}
@{ identity = $identity; version = "1.7.6"; revision = $revision; architecture = "x86_64";
    flavor = "vulkan-cpu-sse2-staticcrt"; source = "https://github.com/ggml-org/whisper.cpp"; files = @($files);
    vulkanSdk = "1.4.321.1 (or VULKAN_SDK override)"; loader = "1.4.321.0"
} | ConvertTo-Json -Depth 5 | Set-Content "$stage/component.json" -Encoding utf8
$backup = "$output.previous"
if (Test-Path $backup) { Remove-Item $backup -Recurse -Force }
if (Test-Path $output) { Move-Item $output $backup }
try { Move-Item $stage $output } catch {
    if (Test-Path $backup) { Move-Item $backup $output }
    throw
}
if (Test-Path $backup) {
    try { Remove-Item $backup -Recurse -Force } catch {
        Write-Warning "新组件已安装；旧组件仍被进程占用，退出转写后可删除 $backup"
    }
}
Write-Host "whisper Vulkan + independent CPU ready: $identity"
