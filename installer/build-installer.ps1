# build-installer.ps1 - 从源码构建产物重建 AMS Studio NSIS 安装包
#
# 用法（推荐先 dot-source 工具链装配脚本）：
#     . 'F:\Deepseek_harness\AI音乐处理\_接管审计\ops\env.ps1'
#     .\build-installer.ps1
#
# 路径规则（本脚本不含任何硬编码盘符 / 用户目录）：
#   - 安装器目录、stage、dist 全部相对本脚本位置（$PSScriptRoot）推导；
#   - launcher 默认取 <repo>\src-tauri\target\release\ams-launcher.exe，
#     可用参数 -LauncherPath 或环境变量 $env:AMS_LAUNCHER_EXE 覆盖；
#   - makensis 优先取参数 -MakensisPath，其次 $env:AMS_MAKENSIS，再其次 PATH；
#     若都没有，请先 dot-source _接管审计\ops\env.ps1。
#
# 行为：
#   1. 校验 launcher FileVersion 与 src-tauri\tauri.conf.json 的 version 一致（防止再次把旧版 stage 打进 setup）；
#   2. 覆盖前把旧 stage\ams-launcher.exe 备份为 *.bak-<时间戳>；
#   3. 运行 makensis（输出名从 ams-setup.nsi 的 OutFile 指令解析，避免硬编码版本名）；
#   4. 校验 setup FileVersion，复制到 installer\dist，并重写 installer-info.json（真实 bytes/sha256）。
#
# 本脚本只做“构建 + 校验 + 刷新产物”，不删除任何文件，不写 C 盘。

[CmdletBinding()]
param(
    [string]$LauncherPath,
    [string]$MakensisPath
)

$ErrorActionPreference = 'Stop'
$installer = $PSScriptRoot
$root      = Split-Path -Parent $installer
$stage     = Join-Path $installer 'stage'
$dist      = Join-Path $installer 'dist'
$nsi       = Join-Path $installer 'ams-setup.nsi'
$tauriConf = Join-Path $root 'src-tauri\tauri.conf.json'
$stamp     = Get-Date -Format 'yyyyMMdd-HHmmss'

# ---------------------------------------------------------------------------
# 1. 解析 launcher / makensis / 期望版本
# ---------------------------------------------------------------------------
if (-not $LauncherPath) { $LauncherPath = $env:AMS_LAUNCHER_EXE }
if (-not $LauncherPath) { $LauncherPath = Join-Path $root 'src-tauri\target\release\ams-launcher.exe' }
if (-not (Test-Path -LiteralPath $LauncherPath)) {
    throw "launcher not found: $LauncherPath（先构建 src-tauri，或 dot-source _接管审计\ops\env.ps1）"
}

if (-not $MakensisPath) { $MakensisPath = $env:AMS_MAKENSIS }
if (-not $MakensisPath) {
    $cmd = Get-Command makensis -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($cmd) { $MakensisPath = $cmd.Source }
}
if (-not $MakensisPath -or -not (Test-Path -LiteralPath $MakensisPath)) {
    throw 'makensis not found：请先 dot-source _接管审计\ops\env.ps1，或用 -MakensisPath 指定'
}

if (-not (Test-Path -LiteralPath $tauriConf)) { throw "tauri.conf.json not found: $tauriConf" }
$expectedVersion = ([string](Get-Content -LiteralPath $tauriConf -Raw | ConvertFrom-Json).version).Trim()
if (-not $expectedVersion) { throw "cannot read version from $tauriConf" }

$nsiText = Get-Content -LiteralPath $nsi -Raw
$outMatch = [regex]::Match($nsiText, '(?m)^\s*OutFile\s+"([^"]+)"')
if (-not $outMatch.Success) { throw "cannot parse OutFile from $nsi" }
# OutFile 里可能使用 ${AMS_VERSION} 占位符，统一展开为 tauri.conf.json 的 version
$setupName = $outMatch.Groups[1].Value.Replace('${AMS_VERSION}', $expectedVersion)
if ($setupName -match '\$\{') { throw "unresolved define in OutFile: $setupName" }
$setupInInstaller = Join-Path $installer $setupName
$setupInDist      = Join-Path $dist $setupName

# ---------------------------------------------------------------------------
# 2. 校验 launcher 版本（核心防回归点：stage 曾残留 0.2.1）
# ---------------------------------------------------------------------------
$launcherItem = Get-Item -LiteralPath $LauncherPath
$launcherVi = $launcherItem.VersionInfo
$launcherVersion = ([string]$launcherVi.FileVersion).Trim()
if ($launcherVersion -ne $expectedVersion) {
    throw ("launcher FileVersion '{0}' != tauri.conf.json version '{1}'（{2}）；请先重建 launcher" -f $launcherVersion, $expectedVersion, $LauncherPath)
}
$launcherHash = (Get-FileHash -LiteralPath $LauncherPath -Algorithm SHA256).Hash
Write-Host ("[ok] launcher : {0}" -f $LauncherPath)
Write-Host ("     version={0} bytes={1} sha256={2}" -f $launcherVersion, $launcherItem.Length, $launcherHash)

# ---------------------------------------------------------------------------
# 2.5 防回归：生产构建必须内嵌前端资源
#   线上 rc.1 launcher 是用 `cargo build --release` 直构建的 dev 模式（未启用
#   tauri custom-protocol 特性），不含 /assets/ 前端资源，装完窗口空白。
#   这里用 dist 里的资源路径作为“已内嵌”证据，防止再次把 dev 构建打进 setup。
# ---------------------------------------------------------------------------
$frontendDist = Join-Path $root 'dist'
if (Test-Path -LiteralPath $frontendDist) {
    $distFiles = @(Get-ChildItem -LiteralPath $frontendDist -Recurse -File -ErrorAction SilentlyContinue)
    $assetKeys = @($distFiles | ForEach-Object { '/' + $_.FullName.Substring($frontendDist.Length + 1).Replace('\', '/') })
    if ($assetKeys.Count -gt 0) {
        $launcherLatin = [System.Text.Encoding]::Latin1.GetString([System.IO.File]::ReadAllBytes($LauncherPath))
        $missing = @($assetKeys | Where-Object { -not $launcherLatin.Contains($_) })
        if ($missing.Count -gt 0) {
            throw ("launcher 未内嵌前端资源（疑似 dev 构建）：缺失 key {0}；必须用 tauri CLI（pnpm tauri build）构建，不能用 cargo build --release" -f ($missing -join ', '))
        }
        Write-Host ("[ok] embedded: {0} 个前端资源 key 已内嵌（{1} ...）" -f $assetKeys.Count, (($assetKeys | Select-Object -First 3) -join ', '))
    }
} else {
    Write-Host '[warn] dist 不存在，跳过前端资源内嵌校验'
}

# ---------------------------------------------------------------------------
# 3. 刷新 stage（覆盖前备份旧文件）
# ---------------------------------------------------------------------------
foreach ($d in @($stage, $dist)) {
    if (-not (Test-Path -LiteralPath $d)) { New-Item -ItemType Directory -Force -Path $d | Out-Null }
}
$stageExe = Join-Path $stage 'ams-launcher.exe'
if (Test-Path -LiteralPath $stageExe) {
    $stageBak = '{0}.bak-{1}' -f $stageExe, $stamp
    Copy-Item -LiteralPath $stageExe -Destination $stageBak -Force
    Write-Host ("[bak] stage   : {0}" -f $stageBak)
}
Copy-Item -LiteralPath $LauncherPath -Destination $stageExe -Force
$stageHash = (Get-FileHash -LiteralPath $stageExe -Algorithm SHA256).Hash
if ($stageHash -ne $launcherHash) { throw "stage copy hash mismatch: $stageHash != $launcherHash" }
Write-Host ("[ok] staged  : {0} sha256={1}" -f $stageExe, $stageHash)

# ---------------------------------------------------------------------------
# 4. 运行 makensis（工作目录必须是 installer，ams-setup.nsi 用相对路径 stage\...）
# ---------------------------------------------------------------------------
if (Test-Path -LiteralPath $setupInInstaller) {
    $setupBak = '{0}.bak-{1}' -f $setupInInstaller, $stamp
    Copy-Item -LiteralPath $setupInInstaller -Destination $setupBak -Force
    Write-Host ("[bak] setup   : {0}" -f $setupBak)
}
Write-Host ("[run] makensis: {0}" -f $MakensisPath)
Push-Location -LiteralPath $installer
try {
    # 版本号唯一来源 = tauri.conf.json 的 version；通过 /D 注入 ams-setup.nsi
    & $MakensisPath "/DAMS_VERSION=$expectedVersion" "/INPUTCHARSET" "UTF8" 'ams-setup.nsi'
    if ($LASTEXITCODE -ne 0) { throw "makensis failed: exit $LASTEXITCODE" }
} finally {
    Pop-Location
}
if (-not (Test-Path -LiteralPath $setupInInstaller)) { throw "installer output missing: $setupInInstaller" }

# ---------------------------------------------------------------------------
# 5. 校验 setup FileVersion（NSIS VIAddVersionKey 写入的文件版本）
# ---------------------------------------------------------------------------
$setupItem = Get-Item -LiteralPath $setupInInstaller
$setupVersion = ([string]$setupItem.VersionInfo.FileVersion).Trim()
if ($setupVersion -ne $expectedVersion) {
    throw ("setup FileVersion '{0}' != expected '{1}'（检查 ams-setup.nsi 的 VIAddVersionKey FileVersion）" -f $setupVersion, $expectedVersion)
}
Write-Host ("[ok] setup   : {0} version={1} bytes={2}" -f $setupInInstaller, $setupVersion, $setupItem.Length)

# ---------------------------------------------------------------------------
# 6. 复制到 dist + 重写 installer-info.json（真实 bytes/sha256）
# ---------------------------------------------------------------------------
if (Test-Path -LiteralPath $setupInDist) {
    $distBak = '{0}.bak-{1}' -f $setupInDist, $stamp
    Copy-Item -LiteralPath $setupInDist -Destination $distBak -Force
    Write-Host ("[bak] dist exe: {0}" -f $distBak)
}
$infoPath = Join-Path $dist 'installer-info.json'
if (Test-Path -LiteralPath $infoPath) {
    $infoBak = '{0}.bak-{1}' -f $infoPath, $stamp
    Copy-Item -LiteralPath $infoPath -Destination $infoBak -Force
    Write-Host ("[bak] info    : {0}" -f $infoBak)
}
Copy-Item -LiteralPath $setupInInstaller -Destination $setupInDist -Force

$distItem = Get-Item -LiteralPath $setupInDist
$info = [pscustomobject]@{
    version = $expectedVersion
    file    = $setupName
    bytes   = $distItem.Length
    sha256  = (Get-FileHash -LiteralPath $setupInDist -Algorithm SHA256).Hash
}
$json = $info | ConvertTo-Json
[System.IO.File]::WriteAllText($infoPath, $json + "`n", (New-Object System.Text.UTF8Encoding($false)))
Write-Host ("[ok] info    : {0}" -f $infoPath)

# ---------------------------------------------------------------------------
# 7. 汇总输出（可被调用方捕获）
# ---------------------------------------------------------------------------
$result = [pscustomobject]@{
    launcher        = $LauncherPath
    launcherVersion = $launcherVersion
    launcherBytes   = $launcherItem.Length
    launcherSha256  = $launcherHash
    setup           = $setupInInstaller
    setupDist       = $setupInDist
    setupVersion    = $setupVersion
    setupBytes      = $info.bytes
    setupSha256     = $info.sha256
    infoPath        = $infoPath
    makensis        = $MakensisPath
}
$result | Format-List | Out-String | Write-Host
Write-Host '=== build-installer done ==='
$result
