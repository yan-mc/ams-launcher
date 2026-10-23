# Legacy AMS (0.1.5) migration helper: back up/remove the old uninstaller so the new setup does a clean install without deleting data.
param(
    [Parameter(Mandatory=$true)][string]$Setup,
    [Parameter(Mandatory=$true)][string]$InstallDir
)
$ErrorActionPreference = "Stop"
$InstallDir = [IO.Path]::GetFullPath($InstallDir)
$backup = Join-Path $InstallDir "data\backups\installer"
New-Item -ItemType Directory -Force -Path $backup | Out-Null
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$regKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\AMS Studio"
$regFile = Join-Path $backup "uninstall-$stamp.reg"
if (Test-Path $regKey) { reg export "HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\AMS Studio" $regFile /y | Out-Null }
Get-Process -Name "ams-launcher" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
$uninstaller = Join-Path $InstallDir "uninstall.exe"
if (Test-Path $uninstaller) {
    $old = Join-Path $backup "uninstall.exe.$stamp"
    Move-Item -LiteralPath $uninstaller -Destination $old -Force
    Write-Host "old uninstaller backed up: $old"
}
if (Test-Path $regKey) { Remove-Item -LiteralPath $regKey -Recurse -Force }
Write-Host "running fresh installer: $Setup"
$p = Start-Process -FilePath $Setup -ArgumentList "/S", "/D=$InstallDir" -PassThru -Wait
if ($p.ExitCode -ne 0) { throw "installer failed: $($p.ExitCode)" }
if (-not (Test-Path (Join-Path $InstallDir "ams-launcher.exe"))) { throw "launcher missing after install" }
Write-Host "migration install OK"