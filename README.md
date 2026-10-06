# ytm-float

Reproductor flotante de YouTube Music para Windows 10. Un exe Win32 en Rust (~270 KB, ~2 MB de RAM)
controla un Brave invisible (headless, perfil propio) por CDP. Brave Shields activo.

Descarga: [bertinilabs.xyz/ytm-float](https://www.bertinilabs.xyz/ytm-float/). Licencia MIT.

## Instalador

Módulos a elección: card de YouTube Music (siempre), Discord en la card, puente a Discord (reemplaza
Kenku FM). Navegador: cualquier Chromium detectado (Brave, Chrome, Edge, Vivaldi, Chromium) o Brave Origin
portátil, que se baja a la carpeta de la app. Solo Brave bloquea anuncios de forma nativa (Shields).
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

## Uso

- Acceso directo: escritorio y menú Inicio → **YTM Float**. Instalado en `%LOCALAPPDATA%\Programs\ytm-float\`.
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
copy target\release\ytm-float.exe %LOCALAPPDATA%\Programs\ytm-float\
```

Cerrar ytm-float antes (el exe en uso no se puede reemplazar).

## Archivos de datos (`%LOCALAPPDATA%\ytm-float\`)

- `perfil\` — perfil de Brave dedicado (sesión de Google, listas de Shields).
- `pos.txt` — posición de la ventana. `brave.pid` — para cerrar un Brave huérfano.
- `engine.log` — errores de sesión CDP. `panic.txt` — último panic de Rust.
