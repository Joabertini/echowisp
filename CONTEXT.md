# CONTEXT — Echowisp, formerly YTM Float (as of 08-10-2026)

## Goal
Ultra-lightweight floating YouTube Music player (free account, no Premium), with a dark card aesthetic
and indigo accent.
It runs with the user's Brave closed. Priorities: minimal resource use and responsiveness.

## Architecture
- `src/main.rs` — UI: layered window (`WS_EX_LAYERED`, `UpdateLayeredWindow`), hand-drawn antialiased
  shapes on a 32-bit DIB, GDI text, custom search field (child controls do not show in layered windows).
  Single instance (mutex), `RegisterHotKey`, `WM_MOUSEACTIVATE` → does not activate except in the search field.
- `src/engine.rs` — CDP session thread: launches Brave, injects `page.js`, receives push events
  (`Runtime.addBinding("__ytmEvt")`), translates responses into `Ev` via `PostMessageW`. Reconnects on its own.
- `src/brave.rs` — finds brave.exe (App Paths / Program Files), launches headless inside a Job object
  with KILL_ON_JOB_CLOSE; checks configuration, assignment, and resumption, and terminates the suspended
  process if it is not confined. Single login window (`--app=accounts.google.com…`), kills orphans
  (only if `perfil\lockfile` is locked and the pid is brave.exe).
- `src/bridge.rs` — bridge in another Job with KILL_ON_JOB_CLOSE. When the card closes, closes stdin to
  send EOF; waits up to 2 s for the bridge to restore volumes and exit, then terminates the Job.
- `src/cdp.rs` — minimal RFC 6455 WebSocket, no dependencies.
- `src/page.js` — runs on music.youtube.com: internal API (`/youtubei/v1/search|browse`), plays via
  `yt-navigate` event (without reloading, ~250 ms; fallback reload), controls by clicking the player bar,
  state through `navigator.mediaSession` + `repeat-mode` from `ytmusic-player-bar`.
- `src/report.rs` — “Report a problem” (at the bottom of Settings): text + optional email + optional
  technical data (Windows and the end of `engine.log` without the user folder; if the session is short,
  completes it with `engine.prev.log`). `engine.log` is per session: on startup it moves to `engine.prev.log`.
  POST via WinHTTP to `reportes.bertinilabs.xyz/v1/reporte` in a thread; returns as `WM_REPORT`. The server stores and forwards it.

## Decisions (with rationale)
- **Playback only through the official player** (option C): audio is played by the YouTube Music page in
  a real browser. Audio is not downloaded or decoded outside the site.
- **No extension**: Brave does not allow extensions to be silently installed outside the store; CDP is sufficient.
- **Brave flags**: `--headless=new --disable-gpu --js-flags=--lite-mode
  --blink-settings=imagesEnabled=false --autoplay-policy=no-user-gesture-required`, with the normal process
  model (sandbox + site isolation). Since 0.5.1, no `--single-process` or disabled isolation: it cost the
  page sandbox ~59 MB private (243 vs 302 playing, 09-10; sandbox without site isolation: 292). Discord:
  `--enable-low-end-device-mode` (749 → 696 MB). DO NOT use `--disable-component-update` or
  `--disable-background-networking`: they leave Shields without lists (measured: 0 vs 4 blocks in 20 s,
  same RAM). DO NOT limit the heap (`--max-old-space-size=96` crashed the tab).
- User agent: headless reports “HeadlessChrome” and YTM rejects it → `Network.setUserAgentOverride`.
- Injection (from when `--single-process` was used; retained): `addScriptToEvaluateOnNewDocument` did not
  always run and a 2nd CDP connection cannot see contexts → inject `page.js` into every main-frame
  `executionContextCreated` and use a single connection. Filter by `frameId == target id` (ad iframes are also isDefault).
- **In-app ad blocking** (`page.js`): removes `adPlacements`/`playerAds`/`adSlots` from each player response
  (`JSON.parse`, `Response.json`, `ytInitialPlayerResponse`) and, if `.ad-showing` still appears, mutes,
  skips to the end, and clicks “Skip”. It does not depend on Shields: lists take time with a new profile.
- **Radio: current + next 5** (`trimQueue`, via `queue.removeItem(String(watchEndpoint.index))`). Only
  forward: removing already played tracks shifts indexes and “next” skips incorrectly. `automixItems`
  (single-song autoplay) is untouched: a trimmed queue is not refilled. Radio does request more on reaching the end.
- **Hourly recycling** (`page.js` → `engine.rs`): YTM retains detached nodes (47k vs 7k alive) and Brave
  does not return RAM on reload; only restarting lowers it (480 → 180 MB measured). After 1 h, while paused
  (≥ 1 min) or right before a song ends, page.js sends `{recycle: {url, t, paused, vol, muted}}`; the exe
  closes the browser, reopens at that URL, and leaves `__ytmRestore` before page.js (position, pause, volume,
  and mute; YTM mute does not survive a restart). The card does not display “Starting” (`quiet`). Tests:
  `window.__ytmRecycleMs = 0` via CDP.
- Browser startup: waits up to 40 s for `DevToolsActivePort` (after updating it can take more than 15).
- **Update from the card** (`update.rs`): on startup reads `www.bertinilabs.xyz/echowisp/version.json`
  (`{version, url, sha256}`). Settings always shows the version row: “checking for updates”, “up to date”,
  “offline (retry)”, or the “Update to X” button. Downloads the installer through WinHTTP (without the
  internet mark: SmartScreen does not block it), verifies SHA-256 (BCrypt), runs it with
  `/VERYSILENT /RELANZAR=1 /NAVEGADOR=<actual>` after ~2 s (cmd + ping: the card must close first because
  of AppMutex), and the installer reopens the card. **When publishing a version: upload the exe and update version.json.**
- Brave Origin discarded: on Windows it is paid (purchase window). Fallback: Edge.
- DevTools HTTP rejects HTTP/1.0 and does not close the connection → HTTP/1.1 + Content-Length.
- `DrawTextW` with an empty string crashes (empty Vec pointer) → skip it.
- Home shelves (`browse FEmusic_home`): find them by es/en title (“Listen again”, “Quick picks”),
  following `nextContinuationData`; quick picks are on page 2 (measured 05-10).
- Ctrl+Alt+M is registered by another user app (measured 05-10 with the card closed) → hide moved to
  Ctrl+Alt+H and collapse to Ctrl+Alt+N. Hotkeys that cannot be registered remain in `engine.log`.
- Task Manager name: version resource (`echowisp.rc`, `FileDescription`) compiled by `build.rs` with
  `embed-resource` (build-dependency only; uses rc.exe from the Windows SDK). Headless Brave is a direct child,
  so it groups underneath.
- Volume through `movie_player.setVolume/mute` (synchronizes with YTM UI), not `video.volume`.
- Width 240 = five controls + margin. Collapsed (double-click, `WM_NCLBUTTONDBLCLK`) measures the text;
  when width changes it retains the center and `pos.txt` saves the expanded card position.

## Measurements
- Discord bridge (`bridge/`), without VB-Cable: WASAPI process loopback by PID/tree, f32 stereo
  48 kHz. Audio path (without its own thread or clock):
  - `process_capture.rs`: one thread per app, wakes on the WASAPI event and writes directly to the mix
    (no intermediate copy).
  - `audio_mix.rs`: queue per app in arrival order; songbird pulls one 20 ms block (`Live`) from its mix
    thread. No apps playing → silence immediately; with apps → waits for the full block from all of them
    until the next 20 ms mark. Queue > 60 ms → trims 2 frames per block (clock drift); 100 ms cap.
  - **Do not align by QPC**: the hourly cursor discarded everything as old if songbird lagged
    (total silence, measured). Packets arrive contiguously (measured deviation 0).
  - `serve.rs` opens and closes the output (`Shared::open/close`) on its thread, in order: an
    asynchronous mixer close would cut the new output on reconnection.
- Ducking (`mixer.rs`): when sending an app, session volume goes to `1e-4` with its own context and
  capture gain to `1e4` (it is post-volume; mute silences it). Original volume per session in
  `ruteo.json` v2; restored on deselection, leave, failure, EOF, and reopening after forced closure.
  Volume change from Windows → that source stops and is not overwritten (tested, expected behavior).
  App check every 1 s; the list is emitted to the card only if it changed.
- **Test audio before requesting a live test**: `echowisp-bridge simular --pid N [--secs S]
  [--espera S] [--out f.raw]` reads through the same path as songbird (RawAdapter → decoder, one packet
  every 20 ms) and counts skips/silence on a tone. `--espera` plays the connected bot before selecting
  the app. Test tone and scripts outside the repo (`notas/tono/`).
- `bridge.log` in `%LOCALAPPDATA%\echowisp\` (previous: `bridge.prev.log`): states, errors, and mix
  counters every 10 s. No token.
- Bot token in `bridge.json`: `token_dpapi` contains base64 DPAPI with user scope, without UI. The bridge
  migrates cleartext `token` on load and replaces the file through `bridge.json.tmp` + rename. If it cannot
  decrypt under another account or PC, it warns the card and remains without a token. Settings → Discord Bot
  → Unlink bot leaves the channel, closes the gateway, and deletes the persisted token.
- Measured live (19045, 1 source): no dropouts, CPU ~0,1 %, 19 MB. Windows 11 and 3 sources remain.
- Build for iteration: `cargo build --profile rapido` (without LTO, ~30 s; release with LTO takes
  20 min with low RAM). Without cmake in PATH, `LIBOPUS_LIB_DIR` = `out` from libopus_sys in
  `target/release/build/` + `LIBOPUS_STATIC=1`.
- `echowisp-bridge audio-probe`: diagnosis without Discord. `IAudioClient2` does not exist in process
  loopback (do not request `POST_VOLUME_LOOPBACK`).
- Exe: ~270 KB; ~2 MB private. Brave (0.5.1, normal model): 8–9 processes, ~300 MB private while playing.

## Pending / ideas
- Test media keys (they may be taken by another app).
- Start with Windows (optional, ask).
- Reposition for per-monitor DPI (`WM_DPICHANGED`).
- A pinned top island was discarded: a draggable floating window was preferred.

## Rename to Echowisp (0.5.0, 08-10)
- “YTM” is a Google trademark. Exes: `echowisp.exe`, `echowisp-bridge.exe`. Data: `%LOCALAPPDATA%\echowisp`.
- Migration: `brave::data_dir()` moves `ytm-float` → `echowisp` once; if it fails, uses the old one and retries.
  The bridge uses whichever exists. Installer: same AppId, `UsePreviousAppDir=no`, removes `Programs\ytm-float`
  and old shortcuts; AppMutex with both names.
- **When publishing: update `/echowisp/version.json` and `/ytm-float/version.json`** (0.4.x reads the old one).
- **Third-party notices:** before publishing, run `scripts/notices.ps1`; it generates
  `THIRD-PARTY-NOTICES.txt` from the normal dependency tree for Windows x64 and the installer distributes
  that file together with `LICENSE`.
