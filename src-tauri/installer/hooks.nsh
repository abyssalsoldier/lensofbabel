; Lens of Babel additions to the NSIS installer.
;
; Included at the top of installer.nsi (bundle > windows > nsis > installerHooks).
; installer.nsi is Tauri's own template with two small changes, marked
; "Lens of Babel", that insert LOB_FINISHPAGE_DEFINES and LOB_FINISHPAGE_FUNCTIONS.

; "Start with Windows" uses the same Run value the app's autostart plugin
; (tauri-plugin-autostart) reads and writes: the value name is the product name,
; so the app's Settings page shows and changes this same setting.
; EDR: writes an HKCU Run key, only when the user ticks the box.
!define LOB_RUNKEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define LOB_STARTUPAPPROVED "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"

; Read the current autostart choice when the installer window opens. This has to
; happen before the "already installed" page, because uninstalling the old
; version there removes the Run value (see NSIS_HOOK_POSTUNINSTALL).
!define MUI_CUSTOMFUNCTION_GUIINIT LobGuiInit

; Goes right before `!insertmacro MUI_PAGE_FINISH`.
!macro LOB_FINISHPAGE_DEFINES
  !define MUI_PAGE_CUSTOMFUNCTION_SHOW LobFinishPageShow
  !define MUI_PAGE_CUSTOMFUNCTION_LEAVE LobFinishPageLeave
!macroend

; Goes after `!insertmacro MUI_PAGE_FINISH`, where the MUI finish page variables exist.
!macro LOB_FINISHPAGE_FUNCTIONS
  Var LobAutostartCheckbox
  Var LobAutostartWasOn

  Function LobGuiInit
    StrCpy $LobAutostartWasOn 0
    ClearErrors
    ReadRegStr $0 HKCU "${LOB_RUNKEY}" "${PRODUCTNAME}"
    ${IfNot} ${Errors}
      StrCpy $LobAutostartWasOn 1
    ${EndIf}
  FunctionEnd

  Function LobFinishPageShow
    ; Below "Run" (90u) and "Create desktop shortcut" (110u), same spacing.
    ${NSD_CreateCheckbox} 120u 130u 195u 10u "Start ${PRODUCTNAME} when Windows starts"
    Pop $LobAutostartCheckbox
    SetCtlColors $LobAutostartCheckbox "${MUI_TEXTCOLOR}" "${MUI_BGCOLOR}"
    ; Keep the current choice when reinstalling or updating; off for a new install.
    ${If} $LobAutostartWasOn = 1
      ${NSD_Check} $LobAutostartCheckbox
    ${EndIf}
  FunctionEnd

  Function LobFinishPageLeave
    ${NSD_GetState} $LobAutostartCheckbox $0
    ${If} $0 = ${BST_CHECKED}
      ; Rewrite it anyway so it points at this install's exe.
      WriteRegStr HKCU "${LOB_RUNKEY}" "${PRODUCTNAME}" '"$INSTDIR\${MAINBINARYNAME}.exe" --autostart'
      ${If} $LobAutostartWasOn <> 1
        ; Newly turned on: clear an old "Disabled" mark from Task Manager, which would win.
        DeleteRegValue HKCU "${LOB_STARTUPAPPROVED}" "${PRODUCTNAME}"
      ${EndIf}
    ${Else}
      DeleteRegValue HKCU "${LOB_RUNKEY}" "${PRODUCTNAME}"
    ${EndIf}
  FunctionEnd
!macroend

; Remove the autostart entry on a real uninstall, but not when the uninstaller
; is run by an update (the new version keeps the user's choice).
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "${LOB_RUNKEY}" "${PRODUCTNAME}"
    DeleteRegValue HKCU "${LOB_STARTUPAPPROVED}" "${PRODUCTNAME}"
    ; The app writes HKLM instead if it was ever run as administrator.
    DeleteRegValue HKLM "${LOB_RUNKEY}" "${PRODUCTNAME}"
    DeleteRegValue HKLM "${LOB_STARTUPAPPROVED}" "${PRODUCTNAME}"
  ${EndIf}
!macroend
