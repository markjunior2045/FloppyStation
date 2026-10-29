; Extra uninstall steps. Tauri's uninstaller already removes the app's entry
; from the Run key (start with Windows); this removes the rest the app leaves
; on this PC. The settings and approved programs in %APPDATA% stay, unless
; the user ticks "delete app data".

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    ; Task Manager's on/off mark for the startup entry.
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "${PRODUCTNAME}"
    ; Logs and the copies the floppy wait helper runs from. A helper still
    ; waiting for a disk keeps its copy until Windows lets it go, so what
    ; can't be deleted now is deleted at the next restart.
    RMDir /r /REBOOTOK "$LOCALAPPDATA\floppy-station"
    ${If} $DeleteAppDataCheckboxState = 1
      RMDir /r "$APPDATA\floppy-station"
    ${EndIf}
  ${EndIf}
!macroend
