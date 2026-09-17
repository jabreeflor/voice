// Main window: Dictations / Snippets / Settings. Talks to the Rust side only
// through the commands and events documented in src/commands.rs.
(() => {
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const { getCurrentWindow } = window.__TAURI__.window;

  const $ = (id) => document.getElementById(id);
  let current = 'dictations';
  let refreshTimer = null;
  let rowSerial = 0;
  // Window showing (per the backend's `window-visible`); event-driven
  // renders are skipped while hidden and replayed on the next show.
  let visible = false;
  let dictationsDirty = true;
  let lastSnippetStamp = null;

  // ── tabs ──────────────────────────────────────────────────────────────
  function select(view) {
    current = view;
    document.querySelectorAll('.tab, .gear').forEach((b) =>
      b.classList.toggle('active', b.dataset.view === view));
    document.querySelectorAll('.view').forEach((v) =>
      v.classList.toggle('visible', v.id === 'view-' + view));
    refresh(true);
  }
  document.querySelectorAll('.tab, .gear').forEach((b) =>
    b.addEventListener('click', () => select(b.dataset.view)));

  // ── dictations ────────────────────────────────────────────────────────
  function formatLatency(seconds) {
    return seconds > 0 ? seconds.toFixed(1) + 's' : '—';
  }

  async function renderDictations() {
    dictationsDirty = false;
    const d = await invoke('get_dictations');
    $('stat-words').textContent = d.total_words.toLocaleString();
    $('stat-wpm').textContent = d.average_wpm > 0 ? String(d.average_wpm) : '—';
    $('stat-latency').textContent = formatLatency(d.average_latency);
    $('live-hotkey').textContent = d.hotkey_short_label;

    const root = $('dictation-groups');
    root.textContent = '';
    if (d.groups.length === 0) {
      const empty = document.createElement('div');
      empty.className = 'sticker empty-card';
      empty.textContent = `Hold ${d.hotkey_short_label} in any app and your dictations will appear here.`;
      root.appendChild(empty);
      return;
    }
    for (const group of d.groups) {
      const cap = document.createElement('div');
      cap.className = 'section-header';
      cap.textContent = group.title;
      root.appendChild(cap);
      const card = document.createElement('div');
      card.className = 'ledger sticker';
      for (const e of group.entries) card.appendChild(ledgerRow(e));
      root.appendChild(card);
    }
  }

  // The row itself is the button, labelled "Copy dictation" with the text as
  // its description and no nested Copy button — the contract LedgerRowTests
  // pins for the AppKit row. Clicking anywhere copies the dictation.
  function ledgerRow(entry) {
    const row = document.createElement('button');
    row.className = 'lrow';
    row.title = 'Click to copy';
    row.setAttribute('aria-label', 'Copy dictation');
    const time = document.createElement('time');
    time.textContent = entry.time;
    const p = document.createElement('p');
    p.id = 'dictation-' + (rowSerial++);
    p.textContent = entry.text;
    row.setAttribute('aria-describedby', p.id);
    const hint = document.createElement('span');
    hint.className = 'hint';
    hint.setAttribute('aria-hidden', 'true');
    hint.textContent = 'Click to copy';
    row.append(time, p, hint);
    // Like LedgerRow.copyToClipboard: 1.4 s accent tint, the hint pops and
    // green confetti bursts out of it. A second click restarts the hold.
    let copiedReset = null;
    row.addEventListener('click', async () => {
      await invoke('copy_text', { text: entry.text });
      hint.textContent = 'Copied';
      row.classList.add('copied');
      popHint(hint);
      burstConfetti(row, hint);
      announce('Copied');
      clearTimeout(copiedReset);
      copiedReset = setTimeout(() => {
        row.classList.remove('copied');
        hint.textContent = 'Click to copy';
      }, 1400);
    });
    return row;
  }

  // 0.86 → 1.12 → 1 scale over 0.28 s (LedgerRow.popCopiedHint). Re-adding
  // the class after a reflow restarts the keyframe on rapid clicks.
  function popHint(hint) {
    hint.classList.remove('pop');
    void hint.offsetWidth;
    hint.classList.add('pop');
  }

  // GreenConfetti.burst: 16 bits, radial 11–24 px, 0.42–0.62 s, gone after 0.7 s.
  const CONFETTI_GREENS = ['#34C759', 'rgb(71,179,112)', 'rgb(41,102,66)', 'rgb(87,199,133)'];
  const rand = (lo, hi) => lo + Math.random() * (hi - lo);
  function burstConfetti(row, hint) {
    row.querySelectorAll('.confetti').forEach((c) => c.remove());
    const host = document.createElement('span');
    host.className = 'confetti';
    host.setAttribute('aria-hidden', 'true');
    host.style.right = (row.clientWidth - hint.offsetLeft - hint.offsetWidth / 2) + 'px';
    for (let i = 0; i < 16; i++) {
      const bit = document.createElement('i');
      const w = rand(2.4, 5.2), h = rand(2.2, 6.0);
      bit.style.width = w + 'px';
      bit.style.height = h + 'px';
      bit.style.borderRadius = Math.random() < 0.5 ? (Math.min(w, h) / 2) + 'px' : '0.7px';
      bit.style.background = CONFETTI_GREENS[i % CONFETTI_GREENS.length];
      const angle = (i / 16) * Math.PI * 2 + rand(-0.28, 0.28);
      const dist = rand(11, 24);
      const dx = Math.cos(angle) * dist, dy = Math.sin(angle) * dist;
      const spin = rand(-2.8, 2.8);
      bit.animate([
        { transform: 'translate(-50%,-50%) rotate(0rad)', opacity: 1, offset: 0 },
        { opacity: 1, offset: 0.35 },
        { transform: `translate(calc(-50% + ${dx}px), calc(-50% + ${dy}px)) rotate(${spin}rad)`, opacity: 0, offset: 1 },
      ], { duration: rand(420, 620), easing: 'ease-out', fill: 'forwards' });
      host.appendChild(bit);
    }
    row.appendChild(host);
    setTimeout(() => host.remove(), 700);
  }

  function announce(text) {
    const live = $('sr-live');
    live.textContent = '';
    live.textContent = text;
  }

  // ── snippets ──────────────────────────────────────────────────────────
  // The DOM is rebuilt only when the store's stamp moved (an app or voicectl
  // edit), so the 1 s poll does not drop hover/focus on the ✕ buttons — the
  // `lastSnippetStamp` guard of the AppKit window. `force` skips the guard
  // when the tab is (re)entered.
  async function renderSnippets(force) {
    const { stamp, snippets: list } = await invoke('get_snippets');
    if (!force && stamp === lastSnippetStamp) return;
    lastSnippetStamp = stamp;
    const root = $('snippet-rows');
    root.textContent = '';
    if (list.length === 0) {
      const empty = document.createElement('div');
      empty.className = 'sempty';
      empty.textContent = 'No snippets yet — add one and say its trigger while dictating.';
      root.appendChild(empty);
    }
    list.forEach((s, index) => {
      const row = document.createElement('div');
      row.className = 'srow';
      const t = document.createElement('span');
      t.className = 't';
      t.textContent = '“' + s.trigger + '”';
      const arrow = document.createElement('span');
      arrow.className = 'arrow';
      arrow.textContent = '→';
      const x = document.createElement('span');
      x.className = 'x';
      x.textContent = s.text;
      x.title = s.text;
      const del = document.createElement('button');
      del.className = 'del';
      del.textContent = '✕';
      del.title = 'Delete snippet';
      del.setAttribute('aria-label', `Delete snippet “${s.trigger}”`);
      del.addEventListener('click', async () => {
        await invoke('remove_snippet', { index });
        renderSnippets();
      });
      row.append(t, arrow, x, del);
      root.appendChild(row);
    });
  }

  $('new-snippet').addEventListener('click', () => {
    $('snew').classList.add('visible');
    $('new-trig').focus();
  });
  async function saveSnippet() {
    const ok = await invoke('add_snippet', { trigger: $('new-trig').value, text: $('new-body').value });
    if (!ok) { $('new-trig').focus(); return; }
    $('new-trig').value = '';
    $('new-body').value = '';
    $('snew').classList.remove('visible');
    renderSnippets();
  }
  $('save-snippet').addEventListener('click', saveSnippet);
  for (const id of ['new-trig', 'new-body']) {
    $(id).addEventListener('keydown', (e) => { if (e.key === 'Enter') saveSnippet(); });
  }

  // ── settings ──────────────────────────────────────────────────────────
  async function renderSettings() {
    const [status, settings] = await Promise.all([invoke('get_status'), invoke('get_settings')]);
    const dot = $('status-dot');
    dot.className = 'dot ' + status.color;
    $('status-text').textContent = status.text;
    $('fix-accessibility').classList.toggle('visible', status.needs_accessibility);

    const sel = $('hotkey-select');
    if (sel.options.length !== settings.hotkeys.length) {
      sel.textContent = '';
      for (const hk of settings.hotkeys) {
        const o = document.createElement('option');
        o.value = hk.id;
        o.textContent = 'Hold ' + hk.label;
        sel.appendChild(o);
      }
    }
    sel.value = settings.hotkey;
    setSwitch($('sounds-switch'), settings.sounds);
    setSwitch($('login-switch'), settings.start_at_login);
  }
  function setSwitch(el, on) {
    el.classList.toggle('on', on);
    el.setAttribute('aria-checked', on ? 'true' : 'false');
  }
  $('hotkey-select').addEventListener('change', (e) => invoke('set_hotkey', { id: e.target.value }));
  $('sounds-switch').addEventListener('click', (e) => {
    const on = !e.currentTarget.classList.contains('on');
    setSwitch(e.currentTarget, on);
    invoke('set_sounds', { enabled: on });
  });
  $('login-switch').addEventListener('click', async (e) => {
    const on = !e.currentTarget.classList.contains('on');
    setSwitch(e.currentTarget, on);
    try { await invoke('set_start_at_login', { enabled: on }); }
    catch (err) { setSwitch(e.currentTarget, !on); console.error(err); }
  });
  $('mic-test').addEventListener('click', () => invoke('preview_mic'));
  // Same as the onboarding "Open Settings" button: make sure Voice is listed
  // in the Accessibility pane, then open it.
  $('fix-accessibility').addEventListener('click', async () => {
    await invoke('request_accessibility');
    await invoke('open_accessibility_settings');
  });

  // ── refresh ───────────────────────────────────────────────────────────
  // Timer tick: the history only changes from inside the app, so the
  // Dictations tab renders on `history-changed` (and when entered) rather
  // than every second, keeping the row's 1.4 s "Copied" hold and keyboard
  // focus intact; the file-backed snippets and the status line still poll.
  function refresh(entering) {
    if (current === 'dictations') {
      if (entering || dictationsDirty) renderDictations();
    } else if (current === 'snippets') {
      renderSnippets(entering);
    } else {
      renderSettings();
    }
  }

  async function init() {
    const settings = await invoke('get_settings');
    document.body.classList.add(settings.platform);
    if (settings.platform === 'macos') {
      $('footnote').textContent = 'Everything runs on this Mac — audio, transcription, history. Nothing leaves your computer.';
    }
    // Event renders are skipped while the window is hidden (a dictation
    // pasted with the window closed would otherwise rebuild an invisible
    // page); the dirty flag replays them on the next show.
    listen('status-changed', () => { if (visible && current === 'settings') renderSettings(); });
    listen('history-changed', () => {
      dictationsDirty = true;
      if (visible && current === 'dictations') renderDictations();
    });
    listen('snippets-changed', () => { if (visible && current === 'snippets') renderSnippets(); });
    // Edits made by voicectl (or by hand) are picked up by the backend on each
    // read, so a 1 s timer keeps the visible tab in sync with the file, the
    // same way the AppKit window did. Like that window's timer it only runs
    // while the window is showing: the backend emits `window-visible` on
    // show/hide (Tauri hides rather than destroys the window on close). The
    // listener goes up before the visibility query so a show in between is
    // not missed. The target is this window's label: `emit_to` is delivered
    // to every `Any`-target listener regardless of label, so a default
    // `listen` would also react to the onboarding window's show/hide.
    listen('window-visible', (e) => setPolling(Boolean(e.payload)),
      { target: getCurrentWindow().label });
    const shown = await getCurrentWindow().isVisible().catch(() => true);
    setPolling(shown);
    select('dictations');
  }
  function setPolling(on) {
    visible = on;
    if (on && !refreshTimer) {
      refreshTimer = setInterval(() => refresh(false), 1000);
      refresh(false);
    } else if (!on && refreshTimer) {
      clearInterval(refreshTimer);
      refreshTimer = null;
    }
  }
  init();
  window.addEventListener('beforeunload', () => setPolling(false));
})();
