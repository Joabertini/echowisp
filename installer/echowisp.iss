; Instalador modular de Echowisp. Compilar: ISCC installer\echowisp.iss (despues de cargo build --release
; en la raiz y en bridge\). Por usuario, sin admin. Navegador: cualquier Chromium detectado (Brave si esta;
; si no, Edge, que viene con Windows). Silencioso: /NAVEGADOR=ruta-al-exe.
#define AppVer "0.6.0"

[Setup]
AppId={{6B1F3C2E-9D4A-4E7B-8C51-2F0A9E7D3B14}
AppName=Echowisp
AppVersion={#AppVer}
AppPublisher=bertinilabs
AppPublisherURL=https://www.bertinilabs.xyz/echowisp/
; Mismo AppId que YTM Float (es una actualizacion), pero carpeta nueva: no reusar la vieja.
DefaultDirName={localappdata}\Programs\echowisp
UsePreviousAppDir=no
DefaultGroupName=Echowisp
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
; La card vieja (0.4.x) usa ytm-float-instancia.
AppMutex=echowisp-instancia,ytm-float-instancia
OutputDir=..\target\instalador
OutputBaseFilename=echowisp-setup-{#AppVer}
UninstallDisplayIcon={app}\echowisp.exe
Compression=lzma2/max
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern
; Idioma segun Windows, sin preguntar.
ShowLanguageDialog=no

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "es"; MessagesFile: "compiler:Languages\Spanish.isl"

[CustomMessages]
en.TipoTodo=Everything
es.TipoTodo=Todo
en.TipoMusica=Music only
es.TipoMusica=Solo música
en.TipoElegir=Custom
es.TipoElegir=Elegir
en.CompMusica=YouTube Music card
es.CompMusica=Card de YouTube Music
en.CompDiscord=Discord in a window next to the card (Discord's own, unmodified)
es.CompDiscord=Discord en una ventana al costado de la card (la de Discord, sin modificar)
en.CompPuente=Discord bridge (app audio → bot, no virtual cable)
es.CompPuente=Puente a Discord (audio de apps → bot, sin cable virtual)
en.TareaInicio=Start with Windows
es.TareaInicio=Abrir con Windows
en.Abrir=Open Echowisp
es.Abrir=Abrir Echowisp
en.NavTitulo=Browser
es.NavTitulo=Navegador
en.NavPregunta=Echowisp plays through a browser with no window. Which one should it use?
es.NavPregunta=Echowisp reproduce con un navegador sin ventana. ¿Cuál uso?
en.NavNota=Echowisp removes ads on its own: it works the same with any of them. If you have no preference, keep the selected one.
es.NavNota=Echowisp saca los anuncios por su cuenta: anda igual con cualquiera. Si no tenés preferencia, dejá el marcado.
en.SinNavegador=No compatible browser found (Brave, Edge, Chrome, Vivaldi or Chromium). Install one and run the installer again.
es.SinNavegador=No encontré ningún navegador compatible (Brave, Edge, Chrome, Vivaldi o Chromium). Instalá uno y volvé a correr el instalador.

[Types]
Name: "full"; Description: "{cm:TipoTodo}"
Name: "musica"; Description: "{cm:TipoMusica}"
Name: "custom"; Description: "{cm:TipoElegir}"; Flags: iscustom

[Components]
Name: "musica"; Description: "{cm:CompMusica}"; Types: full musica custom; Flags: fixed
Name: "discord"; Description: "{cm:CompDiscord}"; Types: full
Name: "puente"; Description: "{cm:CompPuente}"; Types: full

[Tasks]
Name: "inicio"; Description: "{cm:TareaInicio}"; Flags: unchecked

[Files]
Source: "..\target\release\echowisp.exe"; DestDir: "{app}"; Flags: ignoreversion; Components: musica
Source: "discord.module"; DestDir: "{app}"; Components: discord
Source: "..\bridge\target\release\echowisp-bridge.exe"; DestDir: "{app}"; Flags: ignoreversion; Components: puente
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\THIRD-PARTY-NOTICES.txt"; DestDir: "{app}"; Flags: ignoreversion

[InstallDelete]
; Al reinstalar sin un modulo, se saca lo que habia quedado.
Type: files; Name: "{app}\discord.module"; Check: not WizardIsComponentSelected('discord')
Type: files; Name: "{app}\echowisp-bridge.exe"; Check: not WizardIsComponentSelected('puente')
; Brave Origin portatil de versiones viejas: en Windows es pago y abre una ventana de compra.
Type: filesandordirs; Name: "{app}\brave"
; Instalacion de YTM Float (hasta 0.4.x): carpeta y accesos directos viejos.
Type: filesandordirs; Name: "{localappdata}\Programs\ytm-float"
Type: files; Name: "{autoprograms}\YTM Float.lnk"
Type: files; Name: "{userstartup}\YTM Float.lnk"
; El de escritorio lo crea el usuario a mano: apuntaria a la carpeta borrada. Se reemplaza (ver [Icons]).
Type: files; Name: "{userdesktop}\YTM Float.lnk"

[UninstallDelete]
Type: filesandordirs; Name: "{app}\brave"
Type: files; Name: "{app}\navegador.txt"

[Icons]
Name: "{autoprograms}\Echowisp"; Filename: "{app}\echowisp.exe"
Name: "{userstartup}\Echowisp"; Filename: "{app}\echowisp.exe"; Tasks: inicio
Name: "{userdesktop}\Echowisp"; Filename: "{app}\echowisp.exe"; Check: HabiaEscritorio

[Run]
Filename: "{app}\echowisp.exe"; Description: "{cm:Abrir}"; Flags: postinstall nowait skipifsilent
; Actualizacion desde la card (/RELANZAR=1, silenciosa): se vuelve a abrir sola.
Filename: "{app}\echowisp.exe"; Flags: nowait; Check: Relanzar

[Code]
var
  Pagina: TInputOptionWizardPage;
  Rutas: TStringList; { ruta de cada opcion }
  Escritorio: Boolean; { habia acceso directo viejo en el escritorio (se borra antes de crear los nuevos) }

function HabiaEscritorio(): Boolean;
begin
  Result := Escritorio;
end;

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
  Escritorio := FileExists(ExpandConstant('{userdesktop}\YTM Float.lnk'));
  Rutas := TStringList.Create;
  Pagina := CreateInputOptionPage(wpSelectComponents, CustomMessage('NavTitulo'),
    CustomMessage('NavPregunta'), CustomMessage('NavNota'), True, False);
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

function Relanzar(): Boolean;
begin
  Result := ExpandConstant('{param:RELANZAR|0}') = '1';
end;

function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := (PageID = Pagina.ID) and (Rutas.Count < 2);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if Rutas.Count = 0 then
    Result := CustomMessage('SinNavegador');
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    SaveStringToFile(ExpandConstant('{app}\navegador.txt'), Rutas[Pagina.SelectedValueIndex], False);
end;
