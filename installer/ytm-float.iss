; Instalador modular de YTM Float. Compilar: ISCC installer\ytm-float.iss (despues de cargo build --release
; en la raiz y en bridge\). Por usuario, sin admin. Navegador: cualquier Chromium detectado o Brave Origin
; portatil (se baja). Silencioso: /NAVEGADOR=ruta-al-exe o /NAVEGADOR=origin.
#define AppVer "0.1.0"
#define BraveVer "1.96.61"
#define BraveZip "brave-origin-v" + BraveVer + "-win32-x64.zip"
#define BraveSha "97860f4bfa908bd9f4514f5a4d53ed5dcd4093280dd98cef3c7602796025f734"

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
  Bajada: TDownloadWizardPage;
  Rutas: TStringList; { ruta de cada opcion; '' = Brave Origin portatil }

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

function Origin(): Boolean;
begin
  Result := Rutas[Pagina.SelectedValueIndex] = '';
end;

function Elegido(): String;
begin
  if Origin() then Result := ExpandConstant('{app}\brave\brave.exe') else Result := Rutas[Pagina.SelectedValueIndex];
end;

procedure InitializeWizard;
var
  Param: String;
  I: Integer;
begin
  Rutas := TStringList.Create;
  Pagina := CreateInputOptionPage(wpSelectComponents, 'Navegador',
    'YTM Float reproduce con un navegador sin ventana. ¿Cuál uso?',
    'Recomendado: Brave. Es el único con bloqueo de anuncios nativo (Shields); con otro navegador YouTube Music puede mostrar publicidad.',
    True, False);
  Agregar('Brave (recomendado)', Buscar('brave.exe', 'BraveSoftware\Brave-Browser\Application\brave.exe'));
  Agregar('Google Chrome (sin Shields)', Buscar('chrome.exe', 'Google\Chrome\Application\chrome.exe'));
  Agregar('Microsoft Edge (sin Shields)', Buscar('msedge.exe', 'Microsoft\Edge\Application\msedge.exe'));
  Agregar('Vivaldi (sin Shields)', Buscar('vivaldi.exe', 'Vivaldi\Application\vivaldi.exe'));
  Agregar('Chromium (sin Shields)', Buscar('chromium.exe', 'Chromium\Application\chrome.exe'));
  Rutas.Add('');
  Pagina.Add('Bajar Brave Origin portátil, con Shields (208 MB, queda en la carpeta de la app)');
  { Por defecto: Brave si esta; si no, Origin. }
  if Pos('brave', Lowercase(Rutas[0])) > 0 then Pagina.SelectedValueIndex := 0
  else Pagina.SelectedValueIndex := Rutas.Count - 1;
  Param := ExpandConstant('{param:NAVEGADOR|}');
  if CompareText(Param, 'origin') = 0 then Pagina.SelectedValueIndex := Rutas.Count - 1
  else if (Param <> '') and FileExists(Param) then begin
    Agregar(ExtractFileName(Param), Param);
    Pagina.SelectedValueIndex := Rutas.IndexOf(Param);
  end;
  Bajada := CreateDownloadPage('Bajando Brave Origin', 'Se guarda junto a YTM Float; no se instala en el sistema.', nil);
end;

{ Corre tambien en modo silencioso (NextButtonClick no). }
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if not Origin() or FileExists(Elegido()) then Exit;
  Bajada.Clear;
  Bajada.Add('https://github.com/brave/brave-browser/releases/download/v{#BraveVer}/{#BraveZip}', '{#BraveZip}', '{#BraveSha}');
  if not WizardSilent then Bajada.Show;
  try
    try
      Bajada.Download;
    except
      Result := 'No pude bajar Brave Origin: ' + GetExceptionMessage;
    end;
  finally
    if not WizardSilent then Bajada.Hide;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Code: Integer;
  Zip, Dest: String;
begin
  if CurStep <> ssPostInstall then Exit;
  Zip := ExpandConstant('{tmp}\{#BraveZip}');
  if FileExists(Zip) then begin
    Dest := ExpandConstant('{app}\brave');
    ForceDirectories(Dest);
    { tar.exe viene con Windows 10 1803+ y abre zip. }
    if not Exec(ExpandConstant('{sys}\tar.exe'), '-xf "' + Zip + '" -C "' + Dest + '"', '', SW_HIDE, ewWaitUntilTerminated, Code) or (Code <> 0) then
      MsgBox('No pude descomprimir Brave Origin (código ' + IntToStr(Code) + ').', mbError, MB_OK);
  end;
  SaveStringToFile(ExpandConstant('{app}\navegador.txt'), Elegido(), False);
end;
