# Echowisp

Reproductor flotante de YouTube Music para Windows 10, con puente de audio a Discord para partidas de rol (antes YTM Float). Un exe Win32 en Rust (~270 KB, ~2 MB de RAM)
controla un navegador Chromium invisible (headless, perfil propio) por CDP. Los anuncios los saca la app.

Descarga: [bertinilabs.xyz/echowisp](https://www.bertinilabs.xyz/echowisp/). Licencia MIT.

## Instalador

Módulos a elección: card de YouTube Music (siempre), Discord en la card, puente a Discord (reemplaza
Kenku FM). Navegador: cualquier Chromium detectado (Brave, Edge, Chrome, Vivaldi, Chromium); por defecto
Brave si está y si no Edge, que viene con Windows. El bloqueo de anuncios es propio y anda con todos.
Por usuario, sin permisos de administrador.

## Discord: aviso para forks

El módulo de Discord **solo abre Discord web en una ventana normal del navegador**. No inyecta scripts,
no lee el estado interno de Discord, no aprieta botones por el usuario y la ventana se abre sin puerto
de depuración. Lo hicimos así a propósito: automatizar una cuenta de usuario (self-bot) o modificar el
cliente de Discord (scripts, CSS, extensiones) va contra sus
[Términos](https://discord.com/terms) y puede terminar en la **suspensión de la cuenta**.

Si forkeás el proyecto, tené cuidado con `launch_discord` (`src/brave.rs`) y la fila de Discord de la
card: agregar controles "dentro" de la card (mute, ensordecer, canal, quién habla) leyendo o manejando
Discord web pone en riesgo la cuenta de quien lo use. Lo intentamos y lo sacamos por eso. Los flags del
navegador (memoria, GPU) son configuración de nuestro Brave y no tocan Discord.

El puente (`bridge/`) usa un **bot** con su propio token y la API oficial para bots: no usa la cuenta
del usuario.

### Audio de aplicaciones en el puente

El puente captura las apps elegidas por su árbol de procesos con WASAPI process loopback. No requiere
VB-Cable, driver ni permisos de administrador. En la card de apps, activá las fuentes que querés enviar;
su barra regula la ganancia del envío, sin cambiar el volumen que elegiste en Windows. Se pueden mezclar
varias fuentes. El bot solo transmite audio: no recibe las voces del canal.

Mientras una fuente transmite, el puente baja su volumen de sesión en Windows a `0,0001` (−80 dB)
y compensa ese factor en el audio enviado. No usa mute: en Windows 10 22H2 build 19045, la prueba
offline confirmó que mute también silencia la captura. Al deseleccionar, salir o cerrar, restaura el
volumen anterior. Si detecta que el volumen cambió desde Windows durante el envío, detiene esa fuente
y conserva el nuevo valor. `ruteo.json` permite recuperar el volumen tras un cierre forzado y migra
ruteos pendientes de versiones anteriores.

Una app sin sesión de audio compartida activa (incluido el modo exclusivo), una sesión silenciada o
con volumen cero, o un árbol que se solapa con Discord u otra fuente produce un error visible; el
puente no cambia a captura global. Probado en Windows 10 22H2; falta Windows 11.

Si algo suena mal, `%LOCALAPPDATA%\echowisp\bridge.log` registra estados, errores y cada 10 s los
contadores de la mezcla.

## Uso

- Acceso directo: menú Inicio → **Echowisp** (el de escritorio de YTM Float, si existía, se reemplaza). Instalado en `%LOCALAPPDATA%\Programs\echowisp\`.
- Primera vez: ícono de usuario (ámbar) → se abre Brave una sola vez → iniciar sesión en Google → cerrar esa ventana.
- Escribir busca · Enter reproduce · Shift+Enter radio · ↑/↓ elige · Esc limpia · Espacio (buscador vacío) play/pausa · Ctrl+V pega.
- Clic en la barra: salta a ese punto. Arrastrar desde el título mueve la ventana (recuerda la posición).
- Doble clic en canción/artista: colapsa la tarjeta a ese tamaño; otro doble clic la abre.
- Volumen: barra bajo el progreso (clic o arrastre), ícono = silenciar, rueda sobre la tarjeta ±5.
- Botones: aleatorio (mezcla la cola), anterior, play, siguiente, repetir (no → lista → canción).
- Engranaje (arriba a la izquierda): configuración en una card que sale al costado. Apariencia: color de fondo y acento (hex o paleta); la card es translúcida con el fondo desenfocado por Windows. Se guarda en `theme.json`.
- Bajo el buscador: Listas (playlists de la cuenta), Escuchar otra vez y Selección rápida (estantes de la portada). Tocar la activa la cierra.

### Atajos globales (andan dentro de juegos)

| Atajo | Acción |
|---|---|
| Ctrl+Alt+Espacio | Play / pausa |
| Ctrl+Alt+→ / ← | Siguiente / anterior |
| Ctrl+Alt+H | Mostrar / ocultar |
| Ctrl+Alt+N | Colapsar / expandir |
| Ctrl+Alt+B | Mostrar y buscar |
| Teclas multimedia | Igual que arriba (si ninguna otra app las tomó) |

En juegos: usar "pantalla completa en ventana" / sin bordes. En pantalla completa exclusiva solo andan los atajos.

## Compilar

```
cargo build --release
copy target\release\echowisp.exe %LOCALAPPDATA%\Programs\echowisp\
```

Cerrar Echowisp antes (el exe en uso no se puede reemplazar).

## Archivos de datos (`%LOCALAPPDATA%\echowisp\`)

- `perfil\` — perfil dedicado del navegador (sesión de Google).
- `pos.txt` — posición de la ventana. `brave.pid` — para cerrar un Brave huérfano.
- `engine.log` — errores de sesión CDP. `panic.txt` — último panic de Rust.
