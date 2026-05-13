#define AppName "Model Team Tool"
#define AppVersion "0.1.0"
#define AppPublisher "WreckHunter"
#define AppExeName "model-team-tool.exe"

[Setup]
AppId={{B0A8282C-63BF-4F36-A2F5-96D11A1AC8F5}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={autopf}\ModelTeamTool
DefaultGroupName=Model Team Tool
OutputDir=..\dist
OutputBaseFilename=ModelTeamTool-Setup
Compression=lzma
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
Source: "..\target\release\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Model Team Tool"; Filename: "{app}\{#AppExeName}"
Name: "{group}\README"; Filename: "{app}\README.md"

[Run]
Filename: "{app}\{#AppExeName}"; Description: "Run Model Team Tool"; Flags: nowait postinstall skipifsilent
