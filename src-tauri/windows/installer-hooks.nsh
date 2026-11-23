; Register Veloce as a per-user candidate for Windows HTTP/HTTPS defaults.
; Windows retains control of the user's default-app choice.
!macro NSIS_HOOK_POSTINSTALL
  SetRegView 64
  WriteRegStr HKCU "Software\Classes\VeloceURL.HTTP" "" "Veloce HTTP-Adresse"
  WriteRegStr HKCU "Software\Classes\VeloceURL.HTTP" "URL Protocol" ""
  WriteRegStr HKCU "Software\Classes\VeloceURL.HTTP\DefaultIcon" "" "$\"$INSTDIR\veloce-browser.exe$\",0"
  WriteRegStr HKCU "Software\Classes\VeloceURL.HTTP\shell\open\command" "" "$\"$INSTDIR\veloce-browser.exe$\" $\"%1$\""

  WriteRegStr HKCU "Software\Classes\VeloceURL.HTTPS" "" "Veloce HTTPS-Adresse"
  WriteRegStr HKCU "Software\Classes\VeloceURL.HTTPS" "URL Protocol" ""
  WriteRegStr HKCU "Software\Classes\VeloceURL.HTTPS\DefaultIcon" "" "$\"$INSTDIR\veloce-browser.exe$\",0"
  WriteRegStr HKCU "Software\Classes\VeloceURL.HTTPS\shell\open\command" "" "$\"$INSTDIR\veloce-browser.exe$\" $\"%1$\""

  WriteRegStr HKCU "Software\Classes\VeloceHTML" "" "Veloce HTML-Dokument"
  WriteRegStr HKCU "Software\Classes\VeloceHTML\DefaultIcon" "" "$\"$INSTDIR\veloce-browser.exe$\",0"
  WriteRegStr HKCU "Software\Classes\VeloceHTML\shell\open\command" "" "$\"$INSTDIR\veloce-browser.exe$\" $\"%1$\""

  WriteRegStr HKCU "Software\Classes\http\OpenWithProgids" "VeloceURL.HTTP" ""
  WriteRegStr HKCU "Software\Classes\https\OpenWithProgids" "VeloceURL.HTTPS" ""
  WriteRegStr HKCU "Software\Classes\.htm\OpenWithProgids" "VeloceHTML" ""
  WriteRegStr HKCU "Software\Classes\.html\OpenWithProgids" "VeloceHTML" ""

  WriteRegStr HKCU "Software\Classes\Applications\veloce-browser.exe" "FriendlyAppName" "Veloce Browser"
  WriteRegStr HKCU "Software\Classes\Applications\veloce-browser.exe\shell\open\command" "" "$\"$INSTDIR\veloce-browser.exe$\" $\"%1$\""
  WriteRegStr HKCU "Software\Classes\Applications\veloce-browser.exe\SupportedTypes" ".htm" ""
  WriteRegStr HKCU "Software\Classes\Applications\veloce-browser.exe\SupportedTypes" ".html" ""

  WriteRegStr HKCU "Software\Clients\StartMenuInternet\VeloceBrowser" "" "Veloce Browser"
  WriteRegStr HKCU "Software\Clients\StartMenuInternet\VeloceBrowser\DefaultIcon" "" "$\"$INSTDIR\veloce-browser.exe$\",0"
  WriteRegStr HKCU "Software\Clients\StartMenuInternet\VeloceBrowser\shell\open\command" "" "$\"$INSTDIR\veloce-browser.exe$\""
  WriteRegStr HKCU "Software\Clients\StartMenuInternet\VeloceBrowser\Capabilities" "ApplicationName" "Veloce Browser"
  WriteRegStr HKCU "Software\Clients\StartMenuInternet\VeloceBrowser\Capabilities" "ApplicationDescription" "Veloce Browser für HTTP- und HTTPS-Webseiten."
  WriteRegStr HKCU "Software\Clients\StartMenuInternet\VeloceBrowser\Capabilities\StartMenu\StartMenuInternet" "" "veloce-browser.exe"
  WriteRegStr HKCU "Software\Clients\StartMenuInternet\VeloceBrowser\Capabilities\URLAssociations" "http" "VeloceURL.HTTP"
  WriteRegStr HKCU "Software\Clients\StartMenuInternet\VeloceBrowser\Capabilities\URLAssociations" "https" "VeloceURL.HTTPS"
  WriteRegStr HKCU "Software\Clients\StartMenuInternet\VeloceBrowser\Capabilities\FileAssociations" ".htm" "VeloceHTML"
  WriteRegStr HKCU "Software\Clients\StartMenuInternet\VeloceBrowser\Capabilities\FileAssociations" ".html" "VeloceHTML"
  WriteRegStr HKCU "Software\RegisteredApplications" "Veloce Browser" "Software\Clients\StartMenuInternet\VeloceBrowser\Capabilities"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  SetRegView 64
  DeleteRegValue HKCU "Software\RegisteredApplications" "Veloce Browser"
  DeleteRegValue HKCU "Software\Classes\http\OpenWithProgids" "VeloceURL.HTTP"
  DeleteRegValue HKCU "Software\Classes\https\OpenWithProgids" "VeloceURL.HTTPS"
  DeleteRegValue HKCU "Software\Classes\.htm\OpenWithProgids" "VeloceHTML"
  DeleteRegValue HKCU "Software\Classes\.html\OpenWithProgids" "VeloceHTML"
  DeleteRegKey HKCU "Software\Classes\VeloceURL.HTTP"
  DeleteRegKey HKCU "Software\Classes\VeloceURL.HTTPS"
  DeleteRegKey HKCU "Software\Classes\VeloceHTML"
  DeleteRegKey HKCU "Software\Classes\Applications\veloce-browser.exe"
  DeleteRegKey HKCU "Software\Clients\StartMenuInternet\VeloceBrowser"
!macroend
