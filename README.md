# ytm-float

Reproductor flotante de YouTube Music para Windows 10. Un exe Win32 en Rust (~270 KB, ~2 MB de RAM)
controla un Brave invisible (headless, perfil propio) por CDP. Brave Shields activo.

## Uso

- Acceso directo: escritorio y menú Inicio → **YTM Float**. Instalado en `%LOCALAPPDATA%\Programs\ytm-float\`.
- Primera vez: ícono de usuario (ámbar) → se abre Brave una sola vez → iniciar sesión en Google → cerrar esa ventana.
- Escribir busca · Enter reproduce · Shift+Enter radio · ↑/↓ elige · Esc limpia · Espacio (buscador vacío) play/pausa · Ctrl+V pega.
- Clic en la barra: salta a ese punto. Arrastrar desde el título mueve la ventana (recuerda la posición).
- Doble clic en canción/artista: colapsa la tarjeta a ese tamaño; otro doble clic la abre.
- Volumen: barra bajo el progreso (clic o arrastre), ícono = silenciar, rueda sobre la tarjeta ±5.
- Botones: aleatorio (mezcla la cola), anterior, play, siguiente, repetir (no → lista → canción).
- Bajo el buscador: Listas (playlists de la cuenta), Escuchar otra vez y Selección rápida (estantes de la portada). Tocar la activa la cierra.

### Atajos globales (andan dentro de juegos)

| Atajo | Acción |
|---|---|
| Ctrl+Alt+Espacio | Play / pausa |
| Ctrl+Alt+→ / ← | Siguiente / anterior |
| Ctrl+Alt+M | Mostrar / ocultar |
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
