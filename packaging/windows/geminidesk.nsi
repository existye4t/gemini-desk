; GeminiDesk — Per-User Unicode NSIS Installer (Windows 10/11 x64)
;
; Design guarantees:
; - Installs per-user to `%LOCALAPPDATA%\Programs\GeminiDesk` (zero UAC admin prompt).
; - Detects Microsoft Edge WebView2 Runtime cleanly without silently downloading executables.
; - Preserves Google sign-in profile (`%LOCALAPPDATA%\geminidesk\GeminiDesk\data\webview`)
;   and user config (`%APPDATA%\geminidesk\GeminiDesk\config\`) across all upgrades.
; - Uninstall clearly distinguishes app files from optional user data removal (kept by default).

Unicode true
!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

!ifndef PRODUCT_VERSION
  !define PRODUCT_VERSION "1.0.0"
!endif
!ifndef BINARY_PATH
  !define BINARY_PATH "..\..\target\release\gemini-desk.exe"
!endif
!ifndef OUT_FILE
  !define OUT_FILE "..\..\dist\GeminiDesk-${PRODUCT_VERSION}-Windows-x64-Setup.exe"
!endif

!define PRODUCT_NAME "GeminiDesk"
!define PRODUCT_PUBLISHER "GeminiDesk Contributors"
!define PRODUCT_WEB_SITE "https://github.com/existye4t/gemini-desk"
!define PRODUCT_UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_NAME}"
!define WEBVIEW2_GUID "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"

Name "${PRODUCT_NAME}"
OutFile "${OUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\${PRODUCT_NAME}"
InstallDirRegKey HKCU "${PRODUCT_UNINST_KEY}" "InstallLocation"
RequestExecutionLevel user
SetCompressor /SOLID lzma
ShowInstDetails show
ShowUnInstDetails show

VIProductVersion "${PRODUCT_VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCT_NAME}"
VIAddVersionKey "ProductVersion" "${PRODUCT_VERSION}"
VIAddVersionKey "FileVersion" "${PRODUCT_VERSION}.0"
VIAddVersionKey "FileDescription" "GeminiDesk Installer"
VIAddVersionKey "CompanyName" "${PRODUCT_PUBLISHER}"
VIAddVersionKey "LegalCopyright" "MIT OR Apache-2.0"

!define MUI_ICON "..\..\assets\icons\geminidesk.ico"
!define MUI_UNICON "..\..\assets\icons\geminidesk.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\gemini-desk.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Launch GeminiDesk"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\..\THIRD_PARTY_LICENSES.md"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function CheckWebView2Runtime
  ; Check all standard Microsoft Edge WebView2 Runtime Evergreen registry keys
  ReadRegStr $0 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\${WEBVIEW2_GUID}" "pv"
  ${If} $0 != ""
    Return
  ${EndIf}
  ReadRegStr $0 HKLM "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2_GUID}" "pv"
  ${If} $0 != ""
    Return
  ${EndIf}
  ReadRegStr $0 HKCU "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2_GUID}" "pv"
  ${If} $0 != ""
    Return
  ${EndIf}

  ${IfNot} ${Silent}
    MessageBox MB_ICONINFORMATION|MB_YESNO \
      "Microsoft Edge WebView2 Runtime was not detected on this machine.$\r$\n$\r$\nGeminiDesk requires the native Windows WebView2 Runtime to display Google Gemini.$\r$\n$\r$\nOpen Microsoft's official WebView2 download page in your browser?" \
      IDNO done
    ExecShell "open" "https://developer.microsoft.com/en-us/microsoft-edge/webview2/"
  ${EndIf}
  done:
FunctionEnd

Function .onInit
  Call CheckWebView2Runtime
FunctionEnd

Function .onInstSuccess
  ; Support automatic relaunch after in-app verified silent updates (`/S /AUTORESTART`)
  ${If} ${Silent}
    ${GetParameters} $0
    ClearErrors
    ${GetOptions} $0 "/AUTORESTART" $1
    ${IfNot} ${Errors}
      Exec '"$INSTDIR\gemini-desk.exe"'
    ${EndIf}
  ${EndIf}
FunctionEnd

Section "GeminiDesk Core (Required)" SecCore
  SectionIn RO

  ; Gracefully close any running instance before replacing gemini-desk.exe
  nsExec::ExecToLog 'taskkill /IM gemini-desk.exe /F'

  SetOutPath "$INSTDIR"
  File "/oname=gemini-desk.exe" "${BINARY_PATH}"
  File "..\..\THIRD_PARTY_LICENSES.md"
  File "..\..\README.md"

  ; Write uninstaller
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  ; Start Menu shortcuts
  CreateDirectory "$SMPROGRAMS\${PRODUCT_NAME}"
  CreateShortCut "$SMPROGRAMS\${PRODUCT_NAME}\${PRODUCT_NAME}.lnk" "$INSTDIR\gemini-desk.exe" "" "$INSTDIR\gemini-desk.exe" 0
  CreateShortCut "$SMPROGRAMS\${PRODUCT_NAME}\Uninstall ${PRODUCT_NAME}.lnk" "$INSTDIR\Uninstall.exe"

  ; Add/Remove Programs (Per-User HKCU) metadata
  WriteRegStr HKCU "${PRODUCT_UNINST_KEY}" "DisplayName" "${PRODUCT_NAME}"
  WriteRegStr HKCU "${PRODUCT_UNINST_KEY}" "DisplayVersion" "${PRODUCT_VERSION}"
  WriteRegStr HKCU "${PRODUCT_UNINST_KEY}" "Publisher" "${PRODUCT_PUBLISHER}"
  WriteRegStr HKCU "${PRODUCT_UNINST_KEY}" "URLInfoAbout" "${PRODUCT_WEB_SITE}"
  WriteRegStr HKCU "${PRODUCT_UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${PRODUCT_UNINST_KEY}" "DisplayIcon" "$INSTDIR\gemini-desk.exe,0"
  WriteRegStr HKCU "${PRODUCT_UNINST_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${PRODUCT_UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "${PRODUCT_UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${PRODUCT_UNINST_KEY}" "NoRepair" 1

  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${PRODUCT_UNINST_KEY}" "EstimatedSize" "$0"
SectionEnd

Section /o "Desktop Shortcut" SecDesktop
  CreateShortCut "$DESKTOP\${PRODUCT_NAME}.lnk" "$INSTDIR\gemini-desk.exe" "" "$INSTDIR\gemini-desk.exe" 0
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecCore} "Install GeminiDesk executable, Start Menu shortcuts, and uninstaller. Existing Google sign-in and settings are always preserved during upgrades."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "Create an optional shortcut on your Desktop."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  nsExec::ExecToLog 'taskkill /IM gemini-desk.exe /F'

  ; Remove startup registry entry if present
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCT_NAME}"

  ; Remove application files & shortcuts
  Delete "$INSTDIR\gemini-desk.exe"
  Delete "$INSTDIR\THIRD_PARTY_LICENSES.md"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\${PRODUCT_NAME}\${PRODUCT_NAME}.lnk"
  Delete "$SMPROGRAMS\${PRODUCT_NAME}\Uninstall ${PRODUCT_NAME}.lnk"
  RMDir "$SMPROGRAMS\${PRODUCT_NAME}"
  Delete "$DESKTOP\${PRODUCT_NAME}.lnk"

  DeleteRegKey HKCU "${PRODUCT_UNINST_KEY}"

  ; Explicitly ask before removing user settings & Google WebView2 session (KEEP by default).
  ${IfNot} ${Silent}
    MessageBox MB_ICONQUESTION|MB_YESNO|MB_DEFBUTTON2 \
      "Do you also want to remove your saved GeminiDesk settings and Google sign-in profile?$\r$\n$\r$\nChoose No (Recommended) to keep your settings and session for future reinstalls." \
      IDNO skip_userdata
    RMDir /r "$APPDATA\geminidesk\GeminiDesk"
    RMDir /r "$LOCALAPPDATA\geminidesk\GeminiDesk"
    skip_userdata:
  ${EndIf}
SectionEnd
