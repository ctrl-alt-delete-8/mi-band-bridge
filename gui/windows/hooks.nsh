; BandBridge's installer additions (W12), for Tauri's NSIS template.
;
; BandBridge has no start-with-Windows option for now (W11): a person adds it
; to Startup by hand. Tauri's own uninstall still removes a Run value named
; BandBridge, which older test builds wrote, in every run but an /UPDATE.
;
; An uninstall removes the WebView2 data folder Tauri makes for the window,
; and the key where Tauri keeps the install folder, but not in an upgrade:
; see NSIS_HOOK_POSTUNINSTALL. It keeps BandBridge's settings and actions, in
; %APPDATA%\BandBridge, and the band's key, in Credential Manager, unless the
; person ticks "Delete the application data".

!include LogicLib.nsh

; The first page says what this installer does, in place of NSIS's own words
; about closing other programs and restarting, which do not apply here.
!define MUI_WELCOMEPAGE_TEXT "Setup installs Mi Band Bridge for this Windows user.$\r$\n$\r$\nClick Next to continue."

; "Delete the application data" deletes BandBridge's own data too: the band's
; key, and %APPDATA%\BandBridge. BandBridge knows both, from its config, so a
; copy of it does the deleting, with --delete-app-data. The copy is made
; here, while the app's file is still there. It runs in
; NSIS_HOOK_POSTUNINSTALL, once the template has closed BandBridge, so an
; uninstall canceled at that point deletes nothing.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
  ${AndIf} $EXEDIR != $INSTDIR
    InitPluginsDir
    CopyFiles /SILENT "$INSTDIR\${MAINBINARYNAME}.exe" "$PLUGINSDIR\${MAINBINARYNAME}.exe"
  ${EndIf}
!macroend

; Only a real uninstall removes the WebView2 folder and the install-location
; key, and deletes BandBridge's data when the box is ticked. NSIS runs every
; uninstaller from a copy in the temp folder, but an installer that
; uninstalls the old version before it installs runs it in place, in the
; app's folder, and without /UPDATE. So an uninstaller that runs in the app's
; folder is part of an upgrade, and keeps all of it.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
  ${AndIf} $EXEDIR != $INSTDIR
    SetShellVarContext current
    RMDir /r "$LOCALAPPDATA\${BUNDLEID}"
    DeleteRegKey HKCU "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty HKCU "${MANUKEY}"
    ${If} $DeleteAppDataCheckboxState = 1
      Push $0
      ClearErrors
      ExecWait '"$PLUGINSDIR\${MAINBINARYNAME}.exe" --delete-app-data' $0
      ; A failure shows in a box, since the band's key stays behind.
      ${If} ${Errors}
      ${OrIf} $0 <> 0
        MessageBox MB_OK|MB_ICONEXCLAMATION "Mi Band Bridge could not delete its settings and the band's key. They are in %APPDATA%\BandBridge and in Credential Manager." /SD IDOK
      ${EndIf}
      Delete "$PLUGINSDIR\${MAINBINARYNAME}.exe"
      Pop $0
    ${EndIf}
  ${EndIf}
!macroend
