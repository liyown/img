#ifndef Version
  #error Version is required
#endif
[Setup]
AppId=dev.img.desktop
AppName=img
AppVersion={#Version}
DefaultDirName={localappdata}\Programs\img
DefaultGroupName=img
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist\desktop\windows-x86_64
OutputBaseFilename=img-desktop_{#Version}_windows_x86_64
Compression=lzma2
SolidCompression=yes
CloseApplications=yes
RestartApplications=yes
UninstallDisplayIcon={app}\img-desktop.exe
WizardStyle=modern
[Files]
Source: "..\target\desktop\windows-x86_64\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
[Icons]
Name: "{group}\img"; Filename: "{app}\img-desktop.exe"
Name: "{autodesktop}\img"; Filename: "{app}\img-desktop.exe"; Tasks: desktopicon
[Tasks]
Name: desktopicon; Description: "Create a desktop shortcut"; Flags: unchecked
[Run]
Filename: "{app}\img-desktop.exe"; Description: "Launch img"; Flags: nowait postinstall skipifsilent
