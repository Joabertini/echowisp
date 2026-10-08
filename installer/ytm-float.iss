; Instalador modular de YTM Float. Compilar: ISCC installer\ytm-float.iss (despues de cargo build --release
; en la raiz y en bridge\). Por usuario, sin admin. Navegador: cualquier Chromium detectado (Brave si esta;
; si no, Edge, que viene con Windows). Silencioso: /NAVEGADOR=ruta-al-exe.
#define AppVer "0.3.0"

[Setup]
AppId={{6B1F3C2E-9D4A-4E7B-8C51-2F0A9E7D3B14}
AppName=YTM Float
AppVersion={#AppVer}
AppPublisher=bertinilabs
AppPublisherURL=https://www.bertinilabs.xyz/ytm-float/
DefaultDirName={localappdata}\Programs\ytm-float
DefaultGroupName=YTM Float
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
AppMutex=ytm-float-instancia
OutputDir=..\target\instalador
OutputBaseFilename=ytm-float-setup-{#AppVer}
UninstallDisplayIcon={app}\ytm-float.exe
Compression=lzma2/max
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern

[Languages]
Name: "es"; MessagesFile: "compiler:Languages\Spanish.isl"

[Types]
Name: "full"; Description: "Todo"
Name: "musica"; Description: "Solo música"
Name: "custom"; Description: "Elegir"; Flags: iscustom

[Components]
Name: "musica"; Description: "Card de YouTube Music"; Types: full musica custom; Flags: fixed
Name: "discord"; Description: "Discord en una ventana al costado de la card (la de Discord, sin modificar)"; Types: full
Name: "puente"; Description: "Puente a Discord (reemplaza Kenku FM: cable virtual → bot)"; Types: full

[Tasks]
Name: "inicio"; Description: "Abrir con Windows"; Flags: unchecked

[Files]
Source: "..\target\release\ytm-float.exe"; DestDir: "{app}"; Flags: ignoreversion; Components: musica
Source: "discord.module"; DestDir: "{app}"; Components: discord
Source: "..\bridge\target\release\ytm-bridge.exe"; DestDir: "{app}"; Flags: ignoreversion; Components: puente

[InstallDelete]
; Al reinstalar sin un modulo, se saca lo que habia quedado.
Type: files; Name: "{app}\discord.module"; Check: not WizardIsComponentSelected('discord')
Type: files; Name: "{app}\ytm-bridge.exe"; Check: not WizardIsComponentSelected('puente')
; Brave Origin portatil de versiones viejas: en Windows es pago y abre una ventana de compra.
Type: filesandordirs; Name: "{app}\brave"

[UninstallDelete]
Type: filesandordirs; Name: "{app}\brave"
Type: files; Name: "{app}\navegador.txt"

[Icons]
Name: "{autoprograms}\YTM Float"; Filename: "{app}\ytm-float.exe"
Name: "{userstartup}\YTM Float"; Filename: "{app}\ytm-float.exe"; Tasks: inicio

[Run]
Filename: "{app}\ytm-float.exe"; Description: "Abrir YTM Float"; Flags: postinstall nowait skipifsilent

[Code]
var
  Pagina: TInputOptionWizardPage;
  Rutas: TStringList; { ruta de cada opcion }

function AppPath(Root: Integer; Exe: String): String;
begin
  Result := '';
  if RegQueryStringValue(Root, 'SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\' + Exe, '', Result) then
    Result := RemoveQuotes(Result);
end;

{ Primera ruta que exista: App Paths (usuario y equipo) y carpetas tipicas. }
function Buscar(Exe, Sub: String): String;
var
  C: array of String;
  I: Integer;
begin
  C := [AppPath(HKCU, Exe), AppPath(HKLM, Exe),
        ExpandConstant('{localappdata}\') + Sub, ExpandConstant('{commonpf64}\') + Sub, ExpandConstant('{commonpf32}\') + Sub];
  Result := '';
  for I := 0 to GetArrayLength(C) - 1 do
    if (C[I] <> '') and FileExists(C[I]) then begin
      Result := C[I];
      Exit;
    end;
end;

procedure Agregar(Nombre, Ruta: String);
var
  I: Integer;
begin
  if Ruta = '' then Exit;
  for I := 0 to Rutas.Count - 1 do
    if CompareText(Rutas[I], Ruta) = 0 then Exit;
  Rutas.Add(Ruta);
  Pagina.Add(Nombre);
end;

procedure InitializeWizard;
var
  Param: String;
begin
  Rutas := TStringList.Create;
  Pagina := CreateInputOptionPage(wpSelectComponents, 'Navegador',
    'YTM Float reproduce con un navegador sin ventana. ¿Cuál uso?',
    'YTM Float saca los anuncios por su cuenta: anda igual con cualquiera. Si no tenés preferencia, dejá el marcado.',
    True, False);
  { Orden de preferencia: Brave si esta; si no, Edge (viene con Windows). }
  Agregar('Brave', Buscar('brave.exe', 'BraveSoftware\Brave-Browser\Application\brave.exe'));
  Agregar('Microsoft Edge', Buscar('msedge.exe', 'Microsoft\Edge\Application\msedge.exe'));
  Agregar('Google Chrome', Buscar('chrome.exe', 'Google\Chrome\Application\chrome.exe'));
  Agregar('Vivaldi', Buscar('vivaldi.exe', 'Vivaldi\Application\vivaldi.exe'));
  Agregar('Chromium', Buscar('chromium.exe', 'Chromium\Application\chrome.exe'));
  Param := ExpandConstant('{param:NAVEGADOR|}');
  if (Param <> '') and FileExists(Param) then Agregar(ExtractFileName(Param), Param);
  if Rutas.Count > 0 then begin
    Pagina.SelectedValueIndex := 0;
    if Rutas.IndexOf(Param) >= 0 then Pagina.SelectedValueIndex := Rutas.IndexOf(Param);
  end;
end;

function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := (PageID = Pagina.ID) and (Rutas.Count < 2);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if Rutas.Count = 0 then
    Result := 'No encontré ningún navegador compatible (Brave, Edge, Chrome, Vivaldi o Chromium). Instalá uno y volvé a correr el instalador.';
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    SaveStringToFile(ExpandConstant('{app}\navegador.txt'), Rutas[Pagina.SelectedValueIndex], False);
end;
