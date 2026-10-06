; Band App Editor's installer additions (W12), for Tauri's NSIS template.
;
; An uninstall removes the WebView2 data folder Tauri makes for the window,
; as BandBridge's does, and the key where Tauri keeps the install folder, but
; not in an upgrade: see NSIS_HOOK_POSTUNINSTALL. That folder holds the apps
; folder the editor opens again. Your band apps live in the folder you
; picked, and stay.

!include LogicLib.nsh

; The first page says what this installer does, in place of NSIS's own words
; about closing other programs and restarting, which do not apply here.
!define MUI_WELCOMEPAGE_TEXT "Setup installs Mi Band App Editor for this Windows user.$\r$\n$\r$\nClick Next to continue."

; Only a real uninstall removes the WebView2 folder and the install-location
; key. NSIS runs every uninstaller from a copy in the temp folder, but an
; installer that uninstalls the old version before it installs runs it in
; place, in the app's folder, and without /UPDATE. So an uninstaller that
; runs in the app's folder is part of an upgrade, and keeps both.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
  ${AndIf} $EXEDIR != $INSTDIR
    SetShellVarContext current
    RMDir /r "$LOCALAPPDATA\${BUNDLEID}"
    DeleteRegKey HKCU "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty HKCU "${MANUKEY}"
  ${EndIf}
!macroend
