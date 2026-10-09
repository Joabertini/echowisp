# Echowisp

**English** · [Español](README.es.md)

Floating YouTube Music player for Windows 10, with an audio bridge to Discord for tabletop RPG sessions (formerly YTM Float). A Win32 exe in Rust (~270 KB, ~2 MB of RAM)
drives an invisible Chromium browser (headless, its own profile) over CDP. The app removes ads itself.

Download: [bertinilabs.xyz/echowisp](https://www.bertinilabs.xyz/echowisp/). MIT license.

## Installer

Optional modules: YouTube Music card (always), Discord in the card, Discord bridge (replaces
Kenku FM). Browser: any detected Chromium (Brave, Edge, Chrome, Vivaldi, Chromium); by default
Brave if present, otherwise Edge, which ships with Windows. Ad blocking is built in and works with all of them.
Per-user install, no administrator rights.

## Discord: note for forks

The Discord module **only opens Discord web in a normal browser window**. It does not inject scripts,
read Discord's internal state or press buttons for the user, and the window opens without a debugging
port. This is deliberate: automating a user account (self-bot) or modifying the Discord client
(scripts, CSS, extensions) is against Discord's [Terms](https://discord.com/terms) and can get the
**account suspended**.

If you fork the project, be careful with `launch_discord` (`src/brave.rs`) and the card's Discord row:
adding controls "inside" the card (mute, deafen, channel, who is speaking) by reading or driving
Discord web puts the user's account at risk. We tried it and removed it for that reason. The browser
flags (memory, GPU) configure our own Brave and do not touch Discord.

The bridge (`bridge/`) uses a **bot** with its own token and the official bot API: it does not use the
user's account.

### App audio in the bridge

The bridge captures the chosen apps by process tree with WASAPI process loopback. It needs no
VB-Cable, driver or administrator rights. In the apps card, turn on the sources you want to send;
each slider sets the send gain without changing the volume you chose in Windows. Several sources can
be mixed. The bot only sends audio: it does not receive voices from the channel.

While a source is sending, the bridge lowers its Windows session volume to `0.0001` (−80 dB)
and compensates for that factor in the sent audio. It does not use mute: on Windows 10 22H2 build 19045,
offline testing confirmed that mute also silences the capture. On deselect, exit or close, it restores
the previous volume. If the volume is changed from Windows while sending, it stops that source and
keeps the new value. `ruteo.json` restores the volume after a forced shutdown and migrates pending
routes from earlier versions.

An app without an active shared audio session (including exclusive mode), a muted or zero-volume
session, or a process tree that overlaps Discord or another source produces a visible error; the
bridge never falls back to global capture. Tested on Windows 10 22H2; Windows 11 pending.

If something sounds wrong, `%LOCALAPPDATA%\echowisp\bridge.log` records states, errors and, every 10 s,
the mix counters.

## Usage

- Shortcut: Start menu → **Echowisp** (the YTM Float desktop shortcut, if any, is replaced). Installed in `%LOCALAPPDATA%\Programs\echowisp\`.
- First run: user icon (amber) → Brave opens once → sign in to Google → close that window.
- Typing searches · Enter plays · Shift+Enter radio · ↑/↓ select · Esc clears · Space (empty search box) play/pause · Ctrl+V pastes.
- Click on the bar: jump to that point. Drag the title to move the window (position is remembered).
- Double-click on song/artist: collapses the card to that size; double-click again to expand.
- Volume: bar under the progress bar (click or drag), icon = mute, mouse wheel over the card ±5.
- Buttons: shuffle (shuffles the queue), previous, play, next, repeat (off → list → song).
- Gear (top left): settings in a card that opens to the side. Appearance: background and accent color (hex or palette); the card is translucent with the background blurred by Windows. Saved in `theme.json`.
- Under the search box: Playlists (the account's playlists), Listen again and Quick picks (home page shelves). Tapping the active one closes it.

### Global hotkeys (work inside games)

| Hotkey | Action |
|---|---|
| Ctrl+Alt+Space | Play / pause |
| Ctrl+Alt+→ / ← | Next / previous |
| Ctrl+Alt+H | Show / hide |
| Ctrl+Alt+N | Collapse / expand |
| Ctrl+Alt+B | Show and search |
| Media keys | Same as above (if no other app took them) |

In games: use "windowed fullscreen" / borderless. In exclusive fullscreen only the hotkeys work.

## Build

```
cargo build --release
copy target\release\echowisp.exe %LOCALAPPDATA%\Programs\echowisp\
```

Close Echowisp first (an exe in use cannot be replaced).
Before publishing, run `scripts/notices.ps1`.

## Data files (`%LOCALAPPDATA%\echowisp\`)

- `perfil\` — dedicated browser profile (Google session).
- `pos.txt` — window position. `brave.pid` — to close an orphaned Brave.
- `engine.log` — CDP session errors. `panic.txt` — last Rust panic.
- `perfil-discord\` — Discord web session (Discord module). `bridge.json` — bot token and last
  selection (bridge module). `ruteo.json` — original volume of the sent apps.

Uninstalling does not delete this folder: to remove sessions and the token, delete it by hand.

## Privacy

Echowisp has no telemetry and no accounts of its own. It connects to:

- **YouTube Music and Google**, from the browser, with your session (like any browser).
- **Discord**, if you use those modules: the window is Discord web; the bridge uses your bot.
- **www.bertinilabs.xyz**, on launch: reads `echowisp/version.json` to check for a new version.
  It sends no data about you; the server sees your IP as on any visit.
- **reportes.bertinilabs.xyz**, only if you tap "Report a problem": it sends the text you write,
  your email if you leave it and, if you check the box, technical data (Windows version and the end of
  the log, without the user folder). It is stored on the server (Cloudflare) and reaches the developer
  through a private Discord channel. It is used only to fix the problem.

## Code signing policy

The published binaries are built with
[`.github/workflows/release.yml`](.github/workflows/release.yml) from this repository. Free code
signing provided by [SignPath.io](https://signpath.io), certificate by
[SignPath Foundation](https://signpath.org) (applied for; until approved, installers are unsigned).

- Authors (committers) and reviewers: [Joabertini](https://github.com/Joabertini).
- Approvers for each signature: [Joabertini](https://github.com/Joabertini).

Each release is approved by hand. Privacy: see the section above.
