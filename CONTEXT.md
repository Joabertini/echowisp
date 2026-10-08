# CONTEXT — ytm-float (al 04-10-2026)

## Objetivo
Reproductor flotante hiperliviano de YouTube Music (cuenta gratuita, sin Premium), estética oscura
de tarjetas con acento índigo.
Funciona con el Brave del usuario cerrado. Prioridad: recursos mínimos y fluidez.

## Arquitectura
- `src/main.rs` — UI: ventana en capas (`WS_EX_LAYERED`, `UpdateLayeredWindow`), formas antialias
  hechas a mano sobre un DIB de 32 bits, texto GDI, campo de búsqueda propio (los controles hijos no se
  ven en ventanas en capas). Instancia única (mutex), `RegisterHotKey`, `WM_MOUSEACTIVATE` → no activa
  salvo en el buscador.
- `src/engine.rs` — hilo con la sesión CDP: lanza Brave, inyecta `page.js`, recibe eventos push
  (`Runtime.addBinding("__ytmEvt")`), traduce respuestas a `Ev` vía `PostMessageW`. Reconecta solo.
- `src/brave.rs` — busca brave.exe (App Paths / Program Files), lanza headless dentro de un Job object
  con KILL_ON_JOB_CLOSE, ventana de login única (`--app=accounts.google.com…`), mata huérfanos
  (solo si `perfil\lockfile` está bloqueado y el pid es brave.exe).
- `src/cdp.rs` — WebSocket RFC 6455 mínimo, sin dependencias.
- `src/page.js` — corre en music.youtube.com: API interna (`/youtubei/v1/search|browse`), play por
  evento `yt-navigate` (sin recargar, ~250 ms; plan B recarga), controles por clic en la barra del
  reproductor, estado por `navigator.mediaSession` + `repeat-mode` del `ytmusic-player-bar`.
- `src/report.rs` — "Reportar un problema" (al pie de Configuración): texto + mail opcional + datos
  técnicos opcionales (Windows y final de `engine.log` sin la carpeta del usuario; si la sesión es corta,
  completa con `engine.prev.log`). `engine.log` es por sesión: al arrancar pasa a `engine.prev.log`. POST por WinHTTP a
  `reportes.bertinilabs.xyz/v1/reporte` en un hilo; vuelve como `WM_REPORT`. El servidor guarda y reenvía.

## Decisiones (con motivo)
- **No reproducir el audio en nativo** (opción C): YouTube corta la descarga sin token PoToken; el
  clasificador de permisos frenó el intento de evadirlo. Se usa el reproductor oficial en Brave.
- **Sin extensión**: Brave no permite instalar extensiones fuera de la tienda en silencio; CDP alcanza.
- **Flags de Brave**: `--headless=new --single-process --disable-gpu --in-process-gpu
  --js-flags=--lite-mode --blink-settings=imagesEnabled=false --autoplay-policy=no-user-gesture-required`.
  NO usar `--disable-component-update` ni `--disable-background-networking`: dejan a Shields sin
  listas (medido: 0 vs 4 bloqueos en 20 s, misma RAM). NO limitar heap (`--max-old-space-size=96`
  crasheó la pestaña).
- User agent: headless dice "HeadlessChrome" y YTM lo rechaza → `Network.setUserAgentOverride`.
- `--single-process`: `addScriptToEvaluateOnNewDocument` no siempre corre y una 2ª conexión CDP no ve
  contextos → se inyecta `page.js` en cada `executionContextCreated` del frame principal y se usa una
  sola conexión. Filtrar por `frameId == target id` (los iframes de anuncios también son isDefault).
- **Anuncios propios** (`page.js`): se borran `adPlacements`/`playerAds`/`adSlots` de cada respuesta del
  reproductor (`JSON.parse`, `Response.json`, `ytInitialPlayerResponse`) y, si igual aparece `.ad-showing`, se
  silencia, se salta al final y se toca "Omitir". No depende de Shields: con perfil nuevo las listas tardan.
- **Radio: actual + 5 próximas** (`trimQueue`, por `queue.removeItem(String(watchEndpoint.index))`). Solo
  hacia adelante: quitar lo ya sonado corre índices y "siguiente" salta mal. `automixItems` (autoplay de
  canción suelta) no se toca: recortado no se vuelve a llenar. La radio sí pide más al llegar al final.
- Brave Origin descartado: en Windows es pago (ventana de compra). Respaldo: Edge.
- DevTools HTTP rechaza HTTP/1.0 y no cierra la conexión → HTTP/1.1 + Content-Length.
- `DrawTextW` con string vacío revienta (puntero de Vec vacío) → se saltea.
- Estantes de portada (`browse FEmusic_home`): se buscan por título es/en ("Vuelve a escucharlo",
  "Selecciones rápidas") siguiendo `nextContinuationData`; los rápidos vienen en la 2ª página (medido 05-10).
- Ctrl+Alt+M lo tiene registrado otra app del usuario (medido 05-10 con ytm-float cerrado) → ocultar
  pasó a Ctrl+Alt+H y colapsar a Ctrl+Alt+N. Atajos que no se registran quedan en `engine.log`.
- Nombre en el Administrador de tareas: recurso de versión (`ytm-float.rc`, `FileDescription`) compilado
  por `build.rs` con `embed-resource` (solo build-dep; usa rc.exe del Windows SDK). Brave headless es hijo
  directo, así que se agrupa debajo.
- Volumen por `movie_player.setVolume/mute` (sincroniza con la UI de YTM), no `video.volume`.
- Ancho 240 = cinco controles + margen. Colapsada (doble clic, `WM_NCLBUTTONDBLCLK`) mide lo que el texto;
  al cambiar de ancho se conserva el centro y `pos.txt` guarda la posición de la tarjeta expandida.

## Medidas
- Exe: ~270 KB; ~2 MB privados. Brave: 2 procesos, ~180–245 MB reproduciendo; CPU ~0,2 %.

## Pendiente / ideas
- Probar teclas multimedia (pueden estar tomadas por otra app).
- Arranque con Windows (opcional, preguntar).
- Reacomodar con DPI por monitor (`WM_DPICHANGED`).
- Una isla pegada arriba fue descartada: se prefirió flotante arrastrable.
