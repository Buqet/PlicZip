; PlicZip Windows installer.
; Build with:
;   ISCC.exe /DMyAppVersion=1.26.100 /DSourceDir=..\target\release setup\PlicZip.iss

#ifndef MyAppVersion
  #define MyAppVersion "0.0.0-dev"
#endif
#ifndef SourceDir
  #define SourceDir "..\target\release"
#endif

#define MyAppName "PlicZip"
#define MyAppPublisher "PlicZip"
#define MyAppExeName "PlicZip.exe"
#define MyCliExeName "pliczip-cli.exe"
#define MyUpdaterExeName "pliczip-updater.exe"

[Setup]
AppId={{B6B9E0D8-5C6B-4A4B-BF8E-3D5C4B6F1E26}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={localappdata}\PlicZip
DefaultGroupName=PlicZip
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=.
OutputBaseFilename=PlicZip-Setup-v{#MyAppVersion}
Compression=lzma2/ultra64
SolidCompression=yes
ArchitecturesInstallIn64BitMode=x64
ArchitecturesAllowed=x64
WizardStyle=modern
UninstallDisplayName=PlicZip

[Files]
Source: "{#SourceDir}\PlicZip.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\pliczip-cli.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\{#MyUpdaterExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\README.md"; DestDir: "{app}"; Flags: ignoreversion isreadme
Source: "{#SourceDir}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\PlicZip"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"
Name: "{autodesktop}\PlicZip"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Создать ярлык на рабочем столе"; GroupDescription: "Дополнительные ярлыки:"
Name: "addtopath"; Description: "Добавить PlicZip CLI в PATH пользователя"; GroupDescription: "Дополнительные возможности:"; Flags: unchecked

[Registry]
; Association for native PlicZip archives.
Root: HKCU; Subkey: "Software\Classes\.plz\OpenWithProgids"; ValueType: string; ValueName: "PlicZip.Archive"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\PlicZip.Archive"; ValueType: string; ValueData: "PlicZip archive"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\PlicZip.Archive\DefaultIcon"; ValueType: string; ValueData: "{app}\{#MyAppExeName},0"
Root: HKCU; Subkey: "Software\Classes\PlicZip.Archive\shell\open\command"; ValueType: string; ValueData: """{app}\{#MyAppExeName}"" ""%1"""

; Open supported archive formats in PlicZip.
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.plz\shell\PlicZip\command"; ValueType: string; ValueData: """{app}\{#MyAppExeName}"" --context-open ""%1"""; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.zip\shell\PlicZip\command"; ValueType: string; ValueData: """{app}\{#MyAppExeName}"" --context-open ""%1"""; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.7z\shell\PlicZip\command"; ValueType: string; ValueData: """{app}\{#MyAppExeName}"" --context-open ""%1"""; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.rar\shell\PlicZip\command"; ValueType: string; ValueData: """{app}\{#MyAppExeName}"" --context-open ""%1"""; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\SystemFileAssociations\.tar\shell\PlicZip\command"; ValueType: string; ValueData: """{app}\{#MyAppExeName}"" --context-open ""%1"""; Flags: uninsdeletekey

Root: HKCU; Subkey: "Software\Classes\*\shell\PlicZipAdd"; ValueType: string; ValueName: "MUIVerb"; ValueData: "Добавить в архив PlicZip"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\*\shell\PlicZipAdd"; ValueType: string; ValueName: "Icon"; ValueData: "{app}\{#MyAppExeName}"
Root: HKCU; Subkey: "Software\Classes\*\shell\PlicZipAdd\command"; ValueType: string; ValueData: """{app}\{#MyAppExeName}"" --context-add ""%1"""
Root: HKCU; Subkey: "Software\Classes\Directory\shell\PlicZipAdd"; ValueType: string; ValueName: "MUIVerb"; ValueData: "Добавить в архив PlicZip"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Directory\shell\PlicZipAdd"; ValueType: string; ValueName: "Icon"; ValueData: "{app}\{#MyAppExeName}"
Root: HKCU; Subkey: "Software\Classes\Directory\shell\PlicZipAdd\command"; ValueType: string; ValueData: """{app}\{#MyAppExeName}"" --context-add ""%1"""

; Optional user-level PATH entry for the CLI.
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Tasks: addtopath; Check: NeedsAddPath

[Run]
Filename: "{app}\{#MyAppExeName}"; Parameters: "--install-context-menu"; Flags: postinstall skipifsilent nowait

[UninstallRun]
Filename: "{app}\{#MyAppExeName}"; Parameters: "--uninstall-context-menu"; Flags: runhidden waituntilterminated; RunOnceId: "PlicZipRemoveContextMenu"

[Code]
function NeedsAddPath(): Boolean;
var
  PathValue: String;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', PathValue) then
    exit(True);
  Result := Pos(';' + ExpandConstant('{app}') + ';', ';' + PathValue + ';') = 0;
end;

function InitializeSetup(): Boolean;
begin
  Result := True;
  if WizardSilent then
    exit;
  MsgBox('PlicZip будет установлен в профиль текущего пользователя без прав администратора.' + #13#10 +
         'Настройки и portable-версия не будут удалены.', mbInformation, MB_OK);
end;
