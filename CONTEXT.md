# CONTEXT — Echowisp, antes YTM Float (al 08-10-2026)

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
- **Reproducción solo con el reproductor oficial** (opción C): el audio lo reproduce la página de
  YouTube Music en un navegador real. No se descarga ni se decodifica el audio por fuera del sitio.
- **Sin extensión**: Brave no permite instalar extensiones fuera de la tienda en silencio; CDP alcanza.
- **Flags de Brave**: `--headless=new --disable-gpu --js-flags=--lite-mode
  --blink-settings=imagesEnabled=false --autoplay-policy=no-user-gesture-required`, con el modelo de
  procesos normal (sandbox + aislamiento por sitio). Desde 0.5.1 sin `--single-process` ni aislamiento
  apagado: costaba la sandbox de la página por ~59 MB privados (243 vs 302 reproduciendo, 09-10;
  sandbox sin aislamiento por sitio: 292). Discord: `--enable-low-end-device-mode` (749 → 696 MB). NO usar `--disable-component-update` ni `--disable-background-networking`: dejan a Shields sin
  listas (medido: 0 vs 4 bloqueos en 20 s, misma RAM). NO limitar heap (`--max-old-space-size=96`
  crasheó la pestaña).
- User agent: headless dice "HeadlessChrome" y YTM lo rechaza → `Network.setUserAgentOverride`.
- Inyección (de cuando se usaba `--single-process`; se mantiene): `addScriptToEvaluateOnNewDocument` no siempre corría y una 2ª conexión CDP no ve
  contextos → se inyecta `page.js` en cada `executionContextCreated` del frame principal y se usa una
  sola conexión. Filtrar por `frameId == target id` (los iframes de anuncios también son isDefault).
- **Anuncios propios** (`page.js`): se borran `adPlacements`/`playerAds`/`adSlots` de cada respuesta del
  reproductor (`JSON.parse`, `Response.json`, `ytInitialPlayerResponse`) y, si igual aparece `.ad-showing`, se
  silencia, se salta al final y se toca "Omitir". No depende de Shields: con perfil nuevo las listas tardan.
- **Radio: actual + 5 próximas** (`trimQueue`, por `queue.removeItem(String(watchEndpoint.index))`). Solo
  hacia adelante: quitar lo ya sonado corre índices y "siguiente" salta mal. `automixItems` (autoplay de
  canción suelta) no se toca: recortado no se vuelve a llenar. La radio sí pide más al llegar al final.
- **Reciclado horario** (`page.js` → `engine.rs`): YTM retiene nodos desprendidos (47k vs 7k vivos) y Brave
  no devuelve la RAM al recargar; solo reiniciarlo la baja (480 → 180 MB medido). Tras 1 h, en pausa (≥ 1 min)
  o justo antes de terminar una canción, page.js manda `{recycle: {url, t, paused, vol, muted}}`; el exe cierra
  el navegador, reabre en esa URL y deja `__ytmRestore` antes de page.js (posición, pausa, volumen y mute; el
  mute de YTM no sobrevive al reinicio). La card no muestra "Iniciando" (`quiet`). Pruebas:
  `window.__ytmRecycleMs = 0` por CDP.
- Arranque del navegador: espera hasta 40 s el `DevToolsActivePort` (tras actualizarse tarda más de 15).
- **Actualizar desde la card** (`update.rs`): al arrancar lee `www.bertinilabs.xyz/echowisp/version.json`
  (`{version, url, sha256}`). Configuración siempre muestra la fila de versión: "buscando novedades", "al día",
  "sin conexión (reintentar)" o el botón "Actualizar a X". Baja el instalador
  por WinHTTP (sin la marca de internet: SmartScreen no lo frena), verifica SHA-256 (BCrypt), lo corre con
  `/VERYSILENT /RELANZAR=1 /NAVEGADOR=<actual>` tras ~2 s (cmd + ping: la card tiene que cerrarse antes por el
  AppMutex) y el instalador reabre la card. **Al publicar una versión: subir el exe y actualizar version.json.**
- Brave Origin descartado: en Windows es pago (ventana de compra). Respaldo: Edge.
- DevTools HTTP rechaza HTTP/1.0 y no cierra la conexión → HTTP/1.1 + Content-Length.
- `DrawTextW` con string vacío revienta (puntero de Vec vacío) → se saltea.
- Estantes de portada (`browse FEmusic_home`): se buscan por título es/en ("Vuelve a escucharlo",
  "Selecciones rápidas") siguiendo `nextContinuationData`; los rápidos vienen en la 2ª página (medido 05-10).
- Ctrl+Alt+M lo tiene registrado otra app del usuario (medido 05-10 con la card cerrada) → ocultar
  pasó a Ctrl+Alt+H y colapsar a Ctrl+Alt+N. Atajos que no se registran quedan en `engine.log`.
- Nombre en el Administrador de tareas: recurso de versión (`echowisp.rc`, `FileDescription`) compilado
  por `build.rs` con `embed-resource` (solo build-dep; usa rc.exe del Windows SDK). Brave headless es hijo
  directo, así que se agrupa debajo.
- Volumen por `movie_player.setVolume/mute` (sincroniza con la UI de YTM), no `video.volume`.
- Ancho 240 = cinco controles + margen. Colapsada (doble clic, `WM_NCLBUTTONDBLCLK`) mide lo que el texto;
  al cambiar de ancho se conserva el centro y `pos.txt` guarda la posición de la tarjeta expandida.

## Medidas
- Puente a Discord (`bridge/`), sin VB-Cable: WASAPI process loopback por PID/árbol, f32 estéreo
  48 kHz. Camino del audio (sin hilo ni reloj propio):
  - `process_capture.rs`: un hilo por app, despierta con el evento de WASAPI y escribe directo en
    la mezcla (sin copia intermedia).
  - `audio_mix.rs`: cola por app en orden de llegada; songbird tira un bloque de 20 ms (`Live`)
    desde su hilo de mezcla. Sin apps sonando → silencio al instante; con apps → espera el bloque
    completo de todas hasta la próxima marca de 20 ms. Cola > 60 ms → recorta 2 frames por bloque
    (deriva de relojes); tope 100 ms.
  - **No alinear por QPC**: el cursor por hora descartaba todo como viejo si songbird se atrasaba
    (silencio total, medido). Los paquetes llegan contiguos (desvío medido 0).
  - `serve.rs` abre y cierra la salida (`Shared::open/close`) en su hilo, en orden: un cierre
    asíncrono del mezclador cortaba la salida nueva al reconectar.
- Ducking (`mixer.rs`): al enviar una app, volumen de sesión a `1e-4` con contexto propio y
  ganancia `1e4` en la captura (es post volumen; mute la silencia). Volumen original por sesión en
  `ruteo.json` v2; se restaura al deseleccionar, leave, fallo, EOF y al reabrir tras cierre forzado.
  Cambio de volumen desde Windows → se detiene esa fuente y no se pisa (probado, es lo esperado).
  Revisión de apps cada 1 s; la lista se emite a la card solo si cambió.
- **Probar el audio antes de pedir prueba en vivo**: `echowisp-bridge simular --pid N [--secs S]
  [--espera S] [--out f.raw]` lee por el mismo camino que songbird (RawAdapter → decodificador, un
  paquete cada 20 ms) y cuenta saltos/silencio en un tono. `--espera` reproduce el bot conectado
  antes de elegir la app. Tono de prueba y scripts fuera del repo (`notas/tono/`).
- `bridge.log` en `%LOCALAPPDATA%\echowisp\` (anterior: `bridge.prev.log`): estados, errores y
  contadores de la mezcla cada 10 s. Sin token.
- Token del bot en `bridge.json`: `token_dpapi` contiene base64 de DPAPI con ámbito de usuario,
  sin interfaz. El puente migra `token` en claro al cargar y reemplaza el archivo mediante
  `bridge.json.tmp` + renombre. Si no puede descifrarlo en otra cuenta o PC, avisa a la card y
  queda sin token. Configuración → Bot de Discord → Desvincular bot sale del canal, cierra el
  gateway y borra el token persistido.
- Medido en vivo (19045, 1 fuente): sin cortes, CPU ~0,1 %, 19 MB. Falta Win11 y 3 fuentes.
- Compilar para iterar: `cargo build --profile rapido` (sin LTO, ~30 s; release con LTO tarda
  20 min con poca RAM). Sin cmake en el PATH, `LIBOPUS_LIB_DIR` = `out` de libopus_sys en
  `target/release/build/` + `LIBOPUS_STATIC=1`.
- `echowisp-bridge audio-probe`: diagnóstico sin Discord. `IAudioClient2` no existe en process
  loopback (no pedir `POST_VOLUME_LOOPBACK`).
- Exe: ~270 KB; ~2 MB privados. Brave (0.5.1, modelo normal): 8–9 procesos, ~300 MB privados reproduciendo.

## Pendiente / ideas
- Probar teclas multimedia (pueden estar tomadas por otra app).
- Arranque con Windows (opcional, preguntar).
- Reacomodar con DPI por monitor (`WM_DPICHANGED`).
- Una isla pegada arriba fue descartada: se prefirió flotante arrastrable.

## Renombre a Echowisp (0.5.0, 08-10)
- "YTM" es marca de Google. Exes: `echowisp.exe`, `echowisp-bridge.exe`. Datos: `%LOCALAPPDATA%\echowisp`.
- Migración: `brave::data_dir()` mueve `ytm-float` → `echowisp` una vez; si falla, usa la vieja y reintenta. El puente
  usa la que exista. Instalador: mismo AppId, `UsePreviousAppDir=no`, borra `Programs\ytm-float` y accesos viejos;
  AppMutex con los dos nombres.
- **Al publicar: actualizar `/echowisp/version.json` y `/ytm-float/version.json`** (las 0.4.x leen el viejo).
- **Avisos de terceros:** antes de publicar correr `scripts/notices.ps1`; genera
  `THIRD-PARTY-NOTICES.txt` desde el árbol normal de dependencias para Windows x64 y el instalador
  distribuye ese archivo junto con `LICENSE`.
