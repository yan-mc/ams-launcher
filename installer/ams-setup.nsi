Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"

; Version single source: default kept for standalone makensis runs;
; build-installer.ps1 passes /DAMS_VERSION=<version from tauri.conf.json>.
!ifndef AMS_VERSION
  !define AMS_VERSION "1.0.0-rc.2"
!endif

Name "嘉音Studio"
Caption "嘉音Studio 安装程序"
OutFile "AMS-Studio-Setup-${AMS_VERSION}.exe"
InstallDir "F:\AMS-Studio"
InstallDirRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMS Studio" "InstallLocation"
RequestExecutionLevel user
BrandingText "嘉音Studio"
VIProductVersion "1.0.0.0"
VIAddVersionKey "ProductName" "嘉音Studio"
VIAddVersionKey "CompanyName" "yanmc"
VIAddVersionKey "FileDescription" "嘉音Studio 安装程序"
VIAddVersionKey "FileVersion" "${AMS_VERSION}"

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"

Section "AMS Studio" SecMain
  SetOutPath "$INSTDIR"
  File "stage\ams-launcher.exe"

  CreateDirectory "$INSTDIR\data"
  CreateDirectory "$INSTDIR\data\assets"
  CreateDirectory "$INSTDIR\data\artifacts"
  CreateDirectory "$INSTDIR\data\backups"
  CreateDirectory "$INSTDIR\data\cache"
  CreateDirectory "$INSTDIR\data\components"
  CreateDirectory "$INSTDIR\data\config"
  CreateDirectory "$INSTDIR\data\datasets"
  CreateDirectory "$INSTDIR\data\exports"
  CreateDirectory "$INSTDIR\data\kernel"
  CreateDirectory "$INSTDIR\data\logs"
  CreateDirectory "$INSTDIR\data\models"
  CreateDirectory "$INSTDIR\data\projects"
  CreateDirectory "$INSTDIR\data\recordings"
  CreateDirectory "$INSTDIR\data\runtime"
  CreateDirectory "$INSTDIR\data\tmp"
  CreateDirectory "$INSTDIR\data\tools"
  CreateDirectory "$INSTDIR\data\trash"
  CreateDirectory "$INSTDIR\data\tts_voices"
  CreateDirectory "$INSTDIR\data\voices"

  ; Single uninstaller: the launcher detects/opens "$INSTDIR\uninstall.exe".
  ; (Older 1.0.0 installs shipped a byte-identical AMS-Studio-Uninstall.exe copy;
  ; delete it here so an upgrade leaves exactly one uninstaller.)
  Delete "$INSTDIR\AMS-Studio-Uninstall.exe"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMS Studio" "DisplayName" "嘉音Studio"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMS Studio" "DisplayVersion" "${AMS_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMS Studio" "Publisher" "yanmc"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMS Studio" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMS Studio" "DisplayIcon" "$INSTDIR\ams-launcher.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMS Studio" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMS Studio" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMS Studio" "NoRepair" 1
  WriteRegStr HKCU "Software\yanmc\AMS Studio" "" "$INSTDIR"
SectionEnd

Section "Uninstall"
  ; Remove only installer-owned program files. \data is intentionally preserved.
  Delete "$INSTDIR\ams-launcher.exe"
  ; Legacy duplicate from 1.0.0 setups + the single current uninstaller.
  Delete "$INSTDIR\AMS-Studio-Uninstall.exe"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMS Studio"
  DeleteRegKey HKCU "Software\yanmc\AMS Studio"
SectionEnd