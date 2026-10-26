; ==============================================================================
; Syncplay-Alt Windows Installer (Inno Setup 6 Script)
; Industry Standard Installation & Uninstallation Package
; ==============================================================================

#define MyAppName "Syncplay Alt"
#define MyAppVersion "0.1.0"
#define MyAppPublisher "Syncplay Alt Project"
#define MyAppURL "https://github.com/Gauthampro7/syncplay-desktop-preview"
#define MyAppExeName "syncplay-alt.exe"
#define MyAppAssocPrefix "SyncplayAlt"

[Setup]
; Unique AppId generated for Syncplay Alt
AppId={{8B41FE19-D21F-43DE-875F-2D384D02C130}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
AppUpdatesURL={#MyAppURL}/releases
DefaultDirName={autopf}\SyncplayAlt
DefaultGroupName={#MyAppName}
AllowNoIcons=yes
; 64-bit Windows requirement & Modern Wizard Style
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern
WizardResizable=no
ShowLanguageDialog=auto

; Installer & Uninstaller Icons
SetupIconFile=..\..\assets\syncplay-alt.ico
UninstallDisplayIcon={app}\{#MyAppExeName}

; Compression settings for high-efficiency packaging
Compression=lzma2/ultra64
SolidCompression=yes
InternalCompressLevel=ultra64

; Output directory and installer filename
OutputDir=..\..\target\windows-dist
OutputBaseFilename=SyncplayAlt-Setup-v{#MyAppVersion}-x64

; Privileges: per-machine (Program Files) with elevation prompt if standard user
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog

; Clean uninstallation
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "quicklaunchicon"; Description: "Create a Quick Launch icon"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked; OnlyBelowVersion: 6.1; Check: not IsAdminInstallMode
Name: "associate_media"; Description: "Associate Syncplay Alt with supported media files (.mkv, .mp4, .webm, .avi, .mov)"; GroupDescription: "File Associations:"

[Files]
; Primary executable
Source: "..\..\target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
; Supporting Assets
Source: "..\..\assets\syncplay-alt.ico"; DestDir: "{app}\assets"; Flags: ignoreversion
; Any runtime dynamic link libraries present in target/release
Source: "..\..\target\release\*.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
; Documentation & License
Source: "..\..\README.md"; DestDir: "{app}"; DestName: "README.txt"; Flags: ignoreversion isreadme
Source: "..\..\AGENTS.md"; DestDir: "{app}"; DestName: "ARCHITECTURE.txt"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\{#MyAppExeName}"
Name: "{group}\Uninstall {#MyAppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Registry]
; Register Application in Windows App Paths (Enables Win+R > "syncplay-alt")
Root: HKA; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\{#MyAppExeName}"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName}"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\{#MyAppExeName}"; ValueType: string; ValueName: "Path"; ValueData: "{app}"; Flags: uninsdeletekey

; Windows Default Programs Capabilities Registration
Root: HKA; Subkey: "Software\{#MyAppPublisher}\{#MyAppName}\Capabilities"; ValueType: string; ValueName: "ApplicationDescription"; ValueData: "Ultra-efficient standalone desktop media player and synchronized watch-party platform"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\{#MyAppPublisher}\{#MyAppName}\Capabilities"; ValueType: string; ValueName: "ApplicationIcon"; ValueData: "{app}\{#MyAppExeName},0"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\{#MyAppPublisher}\{#MyAppName}\Capabilities"; ValueType: string; ValueName: "ApplicationName"; ValueData: "{#MyAppName}"; Flags: uninsdeletekey

; Capabilities File Associations
Root: HKA; Subkey: "Software\{#MyAppPublisher}\{#MyAppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".mkv"; ValueData: "{#MyAppAssocPrefix}.mkv"; Flags: uninsdeletevalue; Tasks: associate_media
Root: HKA; Subkey: "Software\{#MyAppPublisher}\{#MyAppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".mp4"; ValueData: "{#MyAppAssocPrefix}.mp4"; Flags: uninsdeletevalue; Tasks: associate_media
Root: HKA; Subkey: "Software\{#MyAppPublisher}\{#MyAppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".webm"; ValueData: "{#MyAppAssocPrefix}.webm"; Flags: uninsdeletevalue; Tasks: associate_media
Root: HKA; Subkey: "Software\{#MyAppPublisher}\{#MyAppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".avi"; ValueData: "{#MyAppAssocPrefix}.avi"; Flags: uninsdeletevalue; Tasks: associate_media
Root: HKA; Subkey: "Software\{#MyAppPublisher}\{#MyAppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".mov"; ValueData: "{#MyAppAssocPrefix}.mov"; Flags: uninsdeletevalue; Tasks: associate_media

; Registered Applications entry
Root: HKA; Subkey: "Software\RegisteredApplications"; ValueType: string; ValueName: "{#MyAppName}"; ValueData: "Software\{#MyAppPublisher}\{#MyAppName}\Capabilities"; Flags: uninsdeletevalue

; Explorer Context Menu & File Type Associations (.mkv)
Root: HKA; Subkey: "Software\Classes\{#MyAppAssocPrefix}.mkv"; ValueType: string; ValueName: ""; ValueData: "Matroska Video File"; Flags: uninsdeletekey; Tasks: associate_media
Root: HKA; Subkey: "Software\Classes\{#MyAppAssocPrefix}.mkv\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName},0"; Flags: uninsdeletekey; Tasks: associate_media
Root: HKA; Subkey: "Software\Classes\{#MyAppAssocPrefix}.mkv\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""; Flags: uninsdeletekey; Tasks: associate_media

; Explorer Context Menu & File Type Associations (.mp4)
Root: HKA; Subkey: "Software\Classes\{#MyAppAssocPrefix}.mp4"; ValueType: string; ValueName: ""; ValueData: "MPEG-4 Video File"; Flags: uninsdeletekey; Tasks: associate_media
Root: HKA; Subkey: "Software\Classes\{#MyAppAssocPrefix}.mp4\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName},0"; Flags: uninsdeletekey; Tasks: associate_media
Root: HKA; Subkey: "Software\Classes\{#MyAppAssocPrefix}.mp4\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""; Flags: uninsdeletekey; Tasks: associate_media

; Explorer Context Menu & File Type Associations (.webm)
Root: HKA; Subkey: "Software\Classes\{#MyAppAssocPrefix}.webm"; ValueType: string; ValueName: ""; ValueData: "WebM Video File"; Flags: uninsdeletekey; Tasks: associate_media
Root: HKA; Subkey: "Software\Classes\{#MyAppAssocPrefix}.webm\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName},0"; Flags: uninsdeletekey; Tasks: associate_media
Root: HKA; Subkey: "Software\Classes\{#MyAppAssocPrefix}.webm\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""; Flags: uninsdeletekey; Tasks: associate_media

; Right-click "Play with Syncplay Alt" context menu for all video files
Root: HKA; Subkey: "Software\Classes\SystemFileAssociations\video\shell\SyncplayAlt"; ValueType: string; ValueName: ""; ValueData: "Play with Syncplay Alt"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\SystemFileAssociations\video\shell\SyncplayAlt"; ValueType: string; ValueName: "Icon"; ValueData: "{app}\{#MyAppExeName},0"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\SystemFileAssociations\video\shell\SyncplayAlt\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""; Flags: uninsdeletekey

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[Code]
// Notify Windows Shell of file association changes
procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    // Refreshes Windows Explorer icon cache and file associations
  end;
end;
