; Windows installer (Inno Setup 6). The release workflow builds it with:
;   iscc /DAppVersion=2.1.0 installer\codesprout.iss
; It installs per user, without admin rights, because the app's updater swaps its own
; executable in place and could not do that inside Program Files.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
; Never change AppId: Windows uses it to recognise upgrades and the uninstaller.
AppId={{C7F938E5-E2DB-4D9C-BBE6-89B89840D2BC}
AppName=CodeSprout
AppVersion={#AppVersion}
AppVerName=CodeSprout {#AppVersion}
AppPublisher=Alonso García
AppPublisherURL=https://codesprout.alonsogarcia.dev/
AppSupportURL=https://github.com/alonsoggdev/codesprout/issues
AppUpdatesURL=https://github.com/alonsoggdev/codesprout/releases
PrivilegesRequired=lowest
; With PrivilegesRequired=lowest, {autopf} is %LOCALAPPDATA%\Programs
DefaultDirName={autopf}\CodeSprout
DefaultGroupName=CodeSprout
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
SetupIconFile=..\build\icon.ico
UninstallDisplayIcon={app}\codesprout.exe
UninstallDisplayName=CodeSprout
WizardStyle=modern
OutputDir=..\dist
OutputBaseFilename=CodeSprout-Setup-windows-x64
Compression=lzma2
SolidCompression=yes

[Languages]
Name: "es"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\target\release\codesprout.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\CodeSprout"; Filename: "{app}\codesprout.exe"
Name: "{autodesktop}\CodeSprout"; Filename: "{app}\codesprout.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\codesprout.exe"; Description: "{cm:LaunchProgram,CodeSprout}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Leftover from a self-update that has not been cleaned up yet
Type: files; Name: "{app}\codesprout.old"
