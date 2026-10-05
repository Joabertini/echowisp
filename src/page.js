// Corre dentro de music.youtube.com (Brave headless). El exe llama window.__ytm(cmd, arg)
// por CDP y recibe eventos push por la binding __ytmEvt: sin sondeo desde afuera.
(() => {
  if (window.__ytm || window.top !== window) return; // solo el frame principal
  const cfg = (k) => window.ytcfg?.get?.(k) ?? window.ytcfg?.data_?.[k];
  const player = () => document.getElementById('movie_player');
  const video = () => document.querySelector('video');

  const api = async (ep, body, qs = '') => {
    const headers = { 'Content-Type': 'application/json' };
    const sid = document.cookie.match(/(?:^|; )(?:SAPISID|__Secure-3PAPISID)=([^;]+)/)?.[1];
    if (sid) {
      const ts = Math.floor(Date.now() / 1000);
      const buf = await crypto.subtle.digest('SHA-1', new TextEncoder().encode(`${ts} ${sid} ${location.origin}`));
      const hex = [...new Uint8Array(buf)].map((b) => b.toString(16).padStart(2, '0')).join('');
      headers.Authorization = `SAPISIDHASH ${ts}_${hex}`;
      headers['X-Origin'] = location.origin;
      headers['X-Goog-AuthUser'] = String(cfg('SESSION_INDEX') ?? 0);
    }
    const r = await fetch(`/youtubei/v1/${ep}?prettyPrint=false${qs}`, {
      method: 'POST', headers, credentials: 'include',
      body: JSON.stringify({ context: cfg('INNERTUBE_CONTEXT'), ...body }),
    });
    if (!r.ok) throw new Error(`${ep} HTTP ${r.status}`);
    return r.json();
  };

  // Busca renderers por nombre en cualquier profundidad: aguanta cambios de layout.
  const collect = (o, key, out = []) => {
    if (o && typeof o === 'object') {
      if (o[key]) out.push(o[key]);
      for (const k in o) collect(o[k], key, out);
    }
    return out;
  };
  const txt = (c) => c?.runs?.map((r) => r.text).join('') ?? '';
  const col = (i, n) => txt(i.flexColumns?.[n]?.musicResponsiveListItemFlexColumnRenderer?.text);
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

  const state = () => {
    const v = video(), m = navigator.mediaSession?.metadata, p = player();
    const ad = !!p?.classList?.contains('ad-showing');
    return {
      title: ad ? 'Anuncio' : (m?.title ?? ''), artist: ad ? '' : (m?.artist ?? ''),
      paused: v?.paused ?? true, pos: v?.currentTime ?? 0, dur: (v?.duration > 0 && isFinite(v.duration)) ? v.duration : 0,
      logged: !!cfg('LOGGED_IN'), ad,
      vol: p?.getVolume?.() ?? Math.round((v?.volume ?? 1) * 100), muted: p?.isMuted?.() ?? v?.muted ?? false,
      repeat: document.querySelector('ytmusic-player-bar')?.getAttribute('repeat-mode') ?? 'NONE',
    };
  };

  // ---- Eventos push ----
  let last = '';
  const emit = (force) => {
    const s = state();
    const key = `${s.title}|${s.artist}|${s.paused}|${Math.round(s.dur)}|${s.ad}|${s.repeat}|${s.logged}|${s.vol}|${s.muted}`;
    if (!force && key === last) return;
    last = key;
    try { window.__ytmEvt?.(JSON.stringify(s)); } catch {}
  };
  const hook = () => {
    const v = video();
    if (!v || v.__ytmHooked) return;
    v.__ytmHooked = true;
    for (const e of ['play', 'pause', 'loadedmetadata', 'durationchange', 'ended', 'volumechange']) v.addEventListener(e, () => emit());
    v.addEventListener('seeked', () => emit(true));
  };
  // Chequeo barato cada 1 s: engancha el <video> cuando aparece, detecta cambio de tema
  // y cierra el dialogo "¿Seguis ahi?" que pausa tras horas sin interaccion.
  setInterval(() => {
    hook(); emit();
    document.querySelector('ytmusic-you-there-renderer .yt-spec-button-shape-next, ytmusic-you-there-renderer button')?.click();
  }, 1000);

  const play = async (arg) => {
    const list = arg.radio && arg.v ? 'RDAMVM' + arg.v : arg.list;
    // Se verifica por la URL: durante un anuncio el player reporta el video del anuncio.
    const q = () => new URLSearchParams(location.search);
    const watchEndpoint = {};
    if (arg.v) watchEndpoint.videoId = arg.v;
    if (list) watchEndpoint.playlistId = list;
    document.querySelector('ytmusic-app')?.dispatchEvent(new CustomEvent('yt-navigate', {
      bubbles: true, composed: true, detail: { endpoint: { watchEndpoint } },
    }));
    const done = () => location.pathname === '/watch'
      && (!arg.v || q().get('v') === arg.v) && (!list || q().get('list') === list);
    for (let i = 0; i < 40; i++) { if (done()) return { spa: true }; await sleep(100); }
    const p = new URLSearchParams();
    if (arg.v) p.set('v', arg.v);
    if (list) p.set('list', list);
    setTimeout(() => location.assign('/watch?' + p), 0); // plan B: recarga
    return { spa: false };
  };

  // Estantes de la portada ("Vuelve a escucharlo", "Selecciones rápidas"): se buscan por titulo
  // (es/en) recorriendo las continuaciones; los rapidos suelen estar en la 2a pagina.
  const SHELVES = { again: /vuelve a escuch|volver a escuch|listen again/i, quick: /r[aá]pida|quick pick/i };
  const home = async (which) => {
    let d = await api('browse', { browseId: 'FEmusic_home' });
    for (let k = 0; k < 5 && d; k++) {
      const shelf = collect(d, 'musicCarouselShelfRenderer')
        .find((s) => SHELVES[which].test(txt(s.header?.musicCarouselShelfBasicHeaderRenderer?.title)));
      if (shelf) {
        return shelf.contents.map((it) => {
          const i = it.musicTwoRowItemRenderer ?? it.musicResponsiveListItemRenderer;
          if (!i) return null;
          const id = i.playlistItemData?.videoId ?? collect(i, 'watchEndpoint')[0]?.videoId;
          const list = collect(i, 'watchPlaylistEndpoint')[0]?.playlistId;
          const title = txt(i.title) || col(i, 0), sub = txt(i.subtitle) || col(i, 1);
          // Albumes y listas traen lista: se reproducen enteros; lo demas, como cancion.
          return list ? { list, title, sub } : id ? { id, title, sub } : null;
        }).filter(Boolean);
      }
      const c = collect(d, 'nextContinuationData')[0]?.continuation;
      if (!c) break;
      d = await api('browse', {}, `&ctoken=${c}&continuation=${c}&type=next`);
    }
    return [];
  };

  const cmds = {
    home,
    async search(q) {
      const d = await api('search', { query: q, params: 'EgWKAQIIAWoMEA4QChADEAQQCRAF' }); // filtro: canciones
      return collect(d, 'musicResponsiveListItemRenderer').map((i) => ({
        id: i.playlistItemData?.videoId ?? collect(i, 'watchEndpoint')[0]?.videoId,
        title: col(i, 0), sub: col(i, 1),
      })).filter((x) => x.id).slice(0, 20);
    },
    async playlists() {
      const d = await api('browse', { browseId: 'FEmusic_liked_playlists' });
      return collect(d, 'musicTwoRowItemRenderer').map((i) => {
        const b = i.navigationEndpoint?.browseEndpoint?.browseId;
        return b?.startsWith('VL') ? { list: b.slice(2), title: txt(i.title), sub: txt(i.subtitle) } : null;
      }).filter(Boolean);
    },
    play,
    toggle() {
      const b = document.querySelector('ytmusic-player-bar #play-pause-button');
      if (b) b.click(); else { const v = video(); if (v) v.paused ? v.play() : v.pause(); }
    },
    next() { document.querySelector('ytmusic-player-bar .next-button')?.click(); },
    prev() { document.querySelector('ytmusic-player-bar .previous-button')?.click(); },
    shuffle() { document.querySelector('ytmusic-player-bar .shuffle')?.click(); },
    repeat() { document.querySelector('ytmusic-player-bar .repeat')?.click(); },
    seek(t) { const v = video(); if (v) v.currentTime = t; },
    volume(n) {
      const p = player();
      if (p?.setVolume) { p.setVolume(n); if (n > 0 && p.isMuted?.()) p.unMute(); } else { const v = video(); if (v) v.volume = n / 100; }
      emit();
    },
    mute() {
      const p = player();
      if (p?.isMuted) p.isMuted() ? p.unMute() : p.mute(); else { const v = video(); if (v) v.muted = !v.muted; }
      emit();
    },
    state,
  };

  window.__ytm = async (cmd, arg) => {
    try {
      const f = cmds[cmd];
      if (!f) return { ok: false, error: 'comando desconocido: ' + cmd };
      return { ok: true, data: (await f(arg)) ?? null };
    } catch (e) {
      return { ok: false, error: String(e?.message ?? e) };
    }
  };
  hook(); emit(true);
})();
