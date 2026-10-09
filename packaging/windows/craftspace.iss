; CraftSpace setup for Windows (Inno Setup 6). Installs for the current user, without
; administrator rights, into the same folder CraftSpace installs itself to, so it knows it's the
; installed copy and keeps itself up to date from there.
;
;   iscc /DVersion=0.1.1 /DArch=x64 /DSource=<folder with the .exe files> /O<output folder> craftspace.iss

#ifndef Version
  #define Version "0.0.0"
#endif
#ifndef Arch
  #define Arch "x64"
#endif
#ifndef Source
  #define Source "..\..\target\release"
#endif

[Setup]
AppId={{8C2F6E6A-5B1D-4C51-9F0B-6D3B2A9E7C41}
AppName=CraftSpace
AppVersion={#Version}
AppVerName=CraftSpace {#Version}
AppPublisher=CraftSpace contributors
AppPublisherURL=https://github.com/emircesur/craftspace
AppSupportURL=https://github.com/emircesur/craftspace/issues
AppUpdatesURL=https://github.com/emircesur/craftspace/releases
VersionInfoVersion={#Version}
DefaultDirName={localappdata}\CraftSpace\app
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputBaseFilename=craftspace-{#Version}-windows-{#Arch}-setup
SetupIconFile=..\..\assets\craftspace.ico
UninstallDisplayIcon={app}\bin\craftspace.exe
UninstallDisplayName=CraftSpace
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
CloseApplications=yes
#if Arch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "turkish"; MessagesFile: "compiler:Languages\Turkish.isl"

[CustomMessages]
english.StartAtLogin=Start CraftSpace in the background when I sign in
turkish.StartAtLogin=Oturum açtığımda CraftSpace'i arka planda başlat

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "startup"; Description: "{cm:StartAtLogin}"; Flags: unchecked

[Files]
Source: "{#Source}\craftspace.exe"; DestDir: "{app}\bin"; Flags: ignoreversion
Source: "{#Source}\craftspace-cli.exe"; DestDir: "{app}\bin"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\CraftSpace"; Filename: "{app}\bin\craftspace.exe"
Name: "{userdesktop}\CraftSpace"; Filename: "{app}\bin\craftspace.exe"; Tasks: desktopicon

[Run]
; Through CraftSpace, so its own "start at login" setting matches.
Filename: "{app}\bin\craftspace-cli.exe"; Parameters: "autostart on"; Flags: runhidden; Tasks: startup
Filename: "{app}\bin\craftspace.exe"; Description: "{cm:LaunchProgram,CraftSpace}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{app}\bin\craftspace-cli.exe"; Parameters: "autostart off"; Flags: runhidden; RunOnceId: "AutostartOff"

[UninstallDelete]
; Left behind by self-updates.
Type: files; Name: "{app}\bin\*.old"
Type: files; Name: "{app}\bin\*.new"
