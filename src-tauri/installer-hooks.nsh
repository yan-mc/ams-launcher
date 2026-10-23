; NSIS uninstall hook. Runs before the installer directory is removed.
; Do NOT delete AMS data here: data may contain a large runtime and active
; Python processes. The old cleanup made NSIS upgrades abort after removing
; the launcher. Uninstall intentionally leaves data for safe reinstall/rollback.
!macro NSIS_HOOK_PREUNINSTALL
  ; data is preserved; remove it manually or via a dedicated data-only flow.
!macroend
