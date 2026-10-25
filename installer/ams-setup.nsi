Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"

Name "AMS Studio"
Caption "AMS Studio 安装程序"
OutFile "AMS-Studio-Setup-1.0.0-rc.1.exe"
InstallDir "F:\AMS-Studio"
InstallDirRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMS Studio" "InstallLocation"
RequestExecutionLevel user
BrandingText "AMS Studio"
VIProductVersion "1.0.0.0"
VIAddVersionKey "ProductName" "AMS Studio"
VIAddVersionKey "CompanyName" "yanmc"
VIAddVersionKey "FileDescription" "AMS Studio 安装程序"
VIAddVersionKey "FileVersion" "1.0.0-rc.1"

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

  WriteUninstaller "$INSTDIR\AMS-Studio-Uninstall.exe"

  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMS Studio" "DisplayName" "AMS Studio"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMS Studio" "DisplayVersion" "1.0.0-rc.1"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMS Studio" "Publisher" "yanmc"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMS Studio" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMS Studio" "DisplayIcon" "$INSTDIR\ams-launcher.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMS Studio" "UninstallString" '"$INSTDIR\AMS-Studio-Uninstall.exe"'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMS Studio" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMS Studio" "NoRepair" 1
  WriteRegStr HKCU "Software\yanmc\AMS Studio" "" "$INSTDIR"
SectionEnd

Section "Uninstall"
  ; Remove only installer-owned program files. \data is intentionally preserved.
  Delete "$INSTDIR\ams-launcher.exe"
  Delete "$INSTDIR\AMS-Studio-Uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AMS Studio"
  DeleteRegKey HKCU "Software\yanmc\AMS Studio"
SectionEnd
