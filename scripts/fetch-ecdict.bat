@echo off
rem 下载并解压 ECDICT 完整词典到构建资源目录（已存在则跳过）。
setlocal
set "URL=https://github.com/skywind3000/ECDICT/releases/download/1.0.28/ecdict-sqlite-28.zip"
set "ROOT=%~dp0.."
set "TARGET=%ROOT%\src-tauri\resources\ecdict.db"
set "ZIP=%TEMP%\ecdict-sqlite-28.zip"
set "EXTRACT=%TEMP%\ecdict-sqlite-28"

if exist "%TARGET%" (
  echo ECDICT 词典已存在，跳过下载: %TARGET%
  exit /b 0
)

echo 下载 ECDICT: %URL%
curl.exe -L --fail --retry 2 -o "%ZIP%" "%URL%"
if errorlevel 1 (
  echo 下载 ECDICT 失败
  exit /b 1
)

if exist "%EXTRACT%" rmdir /s /q "%EXTRACT%"
mkdir "%EXTRACT%"
tar.exe -xf "%ZIP%" -C "%EXTRACT%"
if errorlevel 1 (
  echo 解压 ECDICT 失败
  exit /b 1
)

if not exist "%ROOT%\src-tauri\resources" mkdir "%ROOT%\src-tauri\resources"
copy /y "%EXTRACT%\stardict.db" "%TARGET%" >nul
echo ECDICT 词典已就绪: %TARGET%
exit /b 0
