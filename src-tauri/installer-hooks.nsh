; NSIS uninstall hook. Runs before the installer directory is removed.
; The cleanup command only touches AMS-owned kernel/components/runtime/logs.
!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog '"$INSTDIR\ams-launcher.exe" --uninstall-cleanup'
!macroend