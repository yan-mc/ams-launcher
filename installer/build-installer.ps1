$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$installer = Join-Path $root 'installer'
$stage = Join-Path $installer 'stage'
$dist = Join-Path $installer 'dist'
New-Item -ItemType Directory -Force -Path $stage | Out-Null
New-Item -ItemType Directory -Force -Path $dist | Out-Null
$launcher = 'F:\Deepseek_harness\AI音乐处理\08_项目代码\ams-launcher\src-tauri\target\release\ams-launcher.exe'
if (-not (Test-Path -LiteralPath $launcher)) { throw "launcher not found: $launcher" }
Copy-Item -LiteralPath $launcher -Destination (Join-Path $stage 'ams-launcher.exe') -Force
$makensis = 'C:\Users\28988\AppData\Local\tauri\NSIS\makensis.exe'
if (-not (Test-Path -LiteralPath $makensis)) { throw 'makensis not found' }
Push-Location $installer
try {
  & $makensis 'ams-setup.nsi'
  if ($LASTEXITCODE -ne 0) { throw "makensis failed: $LASTEXITCODE" }
  $out = Join-Path $installer 'AMS-Studio-Setup-1.0.0-rc.1.exe'
  if (-not (Test-Path -LiteralPath $out)) { throw 'installer output missing' }
  Copy-Item -LiteralPath $out -Destination (Join-Path $dist 'AMS-Studio-Setup-1.0.0-rc.1.exe') -Force
  $info = [pscustomobject]@{ version = '1.0.0-rc.1'; file = 'AMS-Studio-Setup-1.0.0-rc.1.exe'; bytes = (Get-Item -LiteralPath (Join-Path $dist 'AMS-Studio-Setup-1.0.0-rc.1.exe')).Length; sha256 = (Get-FileHash -LiteralPath (Join-Path $dist 'AMS-Studio-Setup-1.0.0-rc.1.exe') -Algorithm SHA256).Hash }
  $info | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $dist 'installer-info.json') -Encoding UTF8
  $info | Format-List | Out-String
} finally { Pop-Location }
