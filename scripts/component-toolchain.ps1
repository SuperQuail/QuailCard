# Windows whisper 构建工具：只从固定官方地址下载并校验，不修改系统驱动。
$ErrorActionPreference = "Stop"

# 下载先写临时文件，校验通过才供构建使用；失败不接受半成品。
function Get-VerifiedFile([string]$Url, [string]$Path, [string]$Sha256) {
    if ((Test-Path $Path) -and (Get-FileHash $Path -Algorithm SHA256).Hash -eq $Sha256) { return }
    curl.exe --location --fail --retry 3 --retry-all-errors --silent --show-error --output "$Path.partial" $Url
    if ($LASTEXITCODE -ne 0) { throw "组件工具下载失败: $Url" }
    if ((Get-FileHash "$Path.partial" -Algorithm SHA256).Hash -ne $Sha256) { throw "组件工具 SHA256 不匹配" }
    Move-Item -Force "$Path.partial" $Path
}

# 固定版本 CMake 避免上游旧 CMakeLists 被 CMake 4 的兼容性变化破坏。
function Get-ComponentCmake([string]$Work) {
    $cmake = Join-Path $Work "cmake-3.31.8-windows-x86_64/bin/cmake.exe"
    if (-not (Test-Path $cmake)) {
        $zip = Join-Path $Work "cmake.zip"
        Get-VerifiedFile "https://github.com/Kitware/CMake/releases/download/v3.31.8/cmake-3.31.8-windows-x86_64.zip" $zip "81aa9964dbabd71fe02e7ec50472fd3ad56138c49944515ece9001efbff8d719"
        Expand-Archive $zip $Work -Force
    }
    return $cmake
}

# 优先使用显式 SDK；自动准备版只解包头文件、库和工具，不安装/替换显卡驱动。
function Get-ComponentVulkan([string]$Work) {
    if ($env:VULKAN_SDK -and (Test-Path "$env:VULKAN_SDK/Bin/glslc.exe")) { return $env:VULKAN_SDK }
    $sdk = Join-Path $Work "vulkan-sdk"
    if (-not ((Test-Path "$sdk/Bin/glslc.exe") -and (Test-Path "$sdk/Include/vulkan/vulkan.hpp") -and (Test-Path "$sdk/Lib/vulkan-1.lib"))) {
        $archive = Join-Path $Work "vulkan-sdk.exe"
        Get-VerifiedFile "https://sdk.lunarg.com/sdk/download/1.4.321.1/windows/vulkansdk-windows-X64-1.4.321.1.exe" $archive "baaa4f7ca11ed3d82aa1c102b21208915485bbaa473068c763daa425cca468bd"
        $seven = Get-Command 7z.exe -ErrorAction SilentlyContinue
        if (-not $seven) { throw "请安装 7-Zip 或通过 VULKAN_SDK 指定已安装的 Vulkan SDK" }
        python "$PSScriptRoot/component-sdk-extract.py" $archive $sdk $seven.Source | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Vulkan SDK 解包失败；请设置 VULKAN_SDK" }
    }
    return $sdk
}
