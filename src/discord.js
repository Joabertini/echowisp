// Corre dentro de discord.com (pestaña oculta del Brave de ytm-float). La card llama window.__dsc(cmd, arg)
// por CDP y recibe el estado de voz por la binding __dscEvt. Lee los stores internos de Discord (como los
// mods de cliente): mas estable que raspar el DOM. Solo actua cuando el usuario toca un boton en la card.
(() => {
  if (window.__dsc || window.top !== window) return;

  // ---- Acceso a los modulos de Discord (webpack) ----
  let wreq;
  const grab = () => {
    if (wreq) return wreq;
    const chunk = window.webpackChunkdiscord_app;
    if (!chunk?.push) return null;
    chunk.push([[Symbol('ytm')], {}, (r) => { wreq = r; }]);
    chunk.pop();
    return wreq;
  };
  const exportsOf = function* () {
    const c = grab()?.c;
    if (!c) return;
    for (const k in c) {
      const e = c[k]?.exports;
      if (!e || e === window) continue;
      yield e;
      if (typeof e === 'object') for (const p in e) { try { const v = e[p]; if (v && typeof v === 'object' || typeof v === 'function') yield v; } catch {} }
    }
  };
  const cache = {};
  const store = (name) => {
    if (cache[name]) return cache[name];
    for (const e of exportsOf()) { try { if (e?.getName?.() === name) return (cache[name] = e); } catch {} }
    return null;
  };
  const byProps = (...props) => {
    const key = props.join(',');
    if (cache[key]) return cache[key];
    for (const e of exportsOf()) { try { if (e && props.every((p) => typeof e[p] === 'function')) return (cache[key] = e); } catch {} }
    return null;
  };

  const S = {
    sel: () => store('SelectedChannelStore'),
    chan: () => store('ChannelStore'),
    guild: () => store('GuildStore'),
    gchan: () => store('GuildChannelStore'),
    voice: () => store('VoiceStateStore'),
    user: () => store('UserStore'),
    member: () => store('GuildMemberStore'),
    speak: () => store('SpeakingStore'),
    media: () => store('MediaEngineStore'),
  };
  const audio = () => byProps('toggleSelfMute', 'toggleSelfDeaf');
  const chanActions = () => byProps('selectVoiceChannel', 'disconnect');

  const name = (uid, gid) => {
    const u = S.user()?.getUser?.(uid);
    return (gid && S.member()?.getNick?.(gid, uid)) || u?.globalName || u?.username || '?';
  };

  const state = () => {
    const logged = !location.pathname.startsWith('/login') && !!S.user()?.getCurrentUser?.();
    if (!logged) return { logged: false, ready: !!grab() };
    const me = S.user().getCurrentUser();
    const cid = S.sel()?.getVoiceChannelId?.() ?? null;
    const c = cid ? S.chan()?.getChannel?.(cid) : null;
    const gid = c?.guild_id ?? null;
    const vs = cid ? S.voice()?.getVoiceStatesForChannel?.(cid) ?? {} : {};
    const members = Object.values(vs).map((v) => ({
      id: v.userId, name: name(v.userId, gid), me: v.userId === me.id,
      speaking: !!S.speak()?.isSpeaking?.(v.userId), mute: !!(v.selfMute || v.mute), deaf: !!(v.selfDeaf || v.deaf),
    }));
    return {
      logged: true, ready: true, user: name(me.id, gid),
      channel: c ? { id: c.id, name: c.name, guild: S.guild()?.getGuild?.(gid)?.name ?? '' } : null,
      mute: !!S.media()?.isSelfMute?.(), deaf: !!S.media()?.isSelfDeaf?.(), members,
    };
  };

  // ---- Eventos push (agrupados: hablar dispara muchos cambios) ----
  let last = '', timer = 0;
  const emit = () => {
    clearTimeout(timer);
    timer = setTimeout(() => {
      const s = JSON.stringify(state());
      if (s === last) return;
      last = s;
      try { window.__dscEvt?.(s); } catch {}
    }, 120);
  };
  const hooked = new Set();
  const hook = () => {
    for (const k of ['sel', 'voice', 'speak', 'media', 'user']) {
      const st = S[k]();
      if (st && !hooked.has(st) && st.addChangeListener) { st.addChangeListener(emit); hooked.add(st); }
    }
  };
  // Discord carga los stores de a poco: se reintenta hasta engancharlos todos.
  setInterval(() => { hook(); emit(); }, 2000);

  const cmds = {
    state,
    mute() { audio()?.toggleSelfMute(); emit(); },
    deaf() { audio()?.toggleSelfDeaf(); emit(); },
    leave() { chanActions()?.disconnect(); emit(); },
    join(id) { chanActions()?.selectVoiceChannel(id); emit(); },
    // Canales de voz de todos los servidores (para el selector de la card).
    channels() {
      const out = [];
      const guilds = Object.values(S.guild()?.getGuilds?.() ?? {});
      for (const g of guilds) {
        const voc = S.gchan()?.getChannels?.(g.id)?.VOCAL ?? [];
        for (const v of voc) out.push({ id: v.channel.id, name: v.channel.name, guild: g.name });
      }
      return out;
    },
    // Diagnostico: que stores/acciones se encontraron (para cuando Discord cambie algo).
    probe() {
      return Object.fromEntries([...Object.keys(S).map((k) => [k, !!S[k]()]), ['audio', !!audio()], ['chanActions', !!chanActions()]]);
    },
  };

  window.__dsc = async (cmd, arg) => {
    try {
      const f = cmds[cmd];
      if (!f) return { ok: false, error: 'comando desconocido: ' + cmd };
      return { ok: true, data: (await f(arg)) ?? null };
    } catch (e) {
      return { ok: false, error: String(e?.message ?? e) };
    }
  };
  emit();
})();
