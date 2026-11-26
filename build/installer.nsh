!macro customWelcomePage
  !define MUI_WELCOMEPAGE_TITLE "Welcome to Murmint"
  !define MUI_WELCOMEPAGE_TEXT "Murmint types what you say, in any Windows app.$\r$\n$\r$\nHold a shortcut, speak, and clean text appears where your cursor is.$\r$\n$\r$\nThis installer sets up Murmint for your account only. It does not need administrator rights.$\r$\n$\r$\nClick Next to continue."
  !insertmacro MUI_PAGE_WELCOME
!macroend

!macro customUnInstall
  ${ifNot} ${isUpdated}
    ; the downloaded models and the sign-in go with the app, so a reinstall signs in again; an update keeps them
    RMDir /r "$APPDATA\Murmint\models"
    ; Better Clipboard switched Windows clipboard history off; put back the setting it had
    ClearErrors
    ReadRegDWORD $0 HKCU "Software\Murmint" "ClipboardHistoryWas"
    ${ifNot} ${Errors}
      WriteRegDWORD HKCU "Software\Microsoft\Clipboard" "EnableClipboardHistory" $0
    ${endIf}
    DeleteRegValue HKCU "Software\Murmint" "ClipboardHistoryWas"
    Delete "$APPDATA\Murmint\data\account.bin"
    MessageBox MB_YESNO|MB_ICONQUESTION "Also delete your Murmint data?$\r$\n$\r$\nThis removes your settings, history, vocabulary, rules and saved API keys from this PC. Choose No to keep them for a future reinstall." /SD IDNO IDNO murmint_keep_data
      RMDir /r "$APPDATA\Murmint"
      RMDir /r "$TEMP\murmint-setup-webview"
    murmint_keep_data:
  ${endIf}
!macroend
