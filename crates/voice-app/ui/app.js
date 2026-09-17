// Main window: Dictations / Snippets / Settings. Talks to the Rust side only
// through the commands and events documented in src/commands.rs.
(() => {
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;

  const $ = (id) => document.getElementById(id);
  let current = 'dictations';
  let refreshTimer = null;

  // ── tabs ──────────────────────────────────────────────────────────────
  function select(view) {
    current = view;
    document.querySelectorAll('.tab, .gear').forEach((b) =>
      b.classList.toggle('active', b.dataset.view === view));
    document.querySelectorAll('.view').forEach((v) =>
      v.classList.toggle('visible', v.id === 'view-' + view));
    refresh();
  }
  document.querySelectorAll('.tab, .gear').forEach((b) =>
    b.addEventListener('click', () => select(b.dataset.view)));

  // ── dictations ────────────────────────────────────────────────────────
  function formatLatency(seconds) {
    return seconds > 0 ? seconds.toFixed(1) + 's' : '—';
  }

  async function renderDictations() {
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

  // The row itself is the button: clicking anywhere copies the dictation.
  function ledgerRow(entry) {
    const row = document.createElement('button');
    row.className = 'lrow';
    row.title = 'Click to copy';
    row.setAttribute('aria-label', 'Copy dictation');
    const time = document.createElement('time');
    time.textContent = entry.time;
    const p = document.createElement('p');
    p.textContent = entry.text;
    const hint = document.createElement('span');
    hint.className = 'hint';
    hint.textContent = 'Click to copy';
    row.append(time, p, hint);
    row.addEventListener('click', async () => {
      await invoke('copy_text', { text: entry.text });
      hint.textContent = 'Copied';
      row.classList.add('copied');
      setTimeout(() => { row.classList.remove('copied'); hint.textContent = 'Click to copy'; }, 1200);
    });
    return row;
  }

  // ── snippets ──────────────────────────────────────────────────────────
  async function renderSnippets() {
    const list = await invoke('get_snippets');
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
  $('fix-accessibility').addEventListener('click', async () => {
    await invoke('request_accessibility');
    await invoke('open_accessibility_settings');
  });

  // ── refresh ───────────────────────────────────────────────────────────
  function refresh() {
    if (current === 'dictations') renderDictations();
    else if (current === 'snippets') renderSnippets();
    else renderSettings();
  }

  async function init() {
    const settings = await invoke('get_settings');
    document.body.classList.add(settings.platform);
    listen('status-changed', () => { if (current === 'settings') renderSettings(); });
    listen('history-changed', () => { if (current === 'dictations') renderDictations(); });
    listen('snippets-changed', () => { if (current === 'snippets') renderSnippets(); });
    // Edits made by voicectl (or by hand) are picked up by the backend on each
    // read, so a 1 s timer keeps the visible tab in sync with the file, the
    // same way the AppKit window did.
    refreshTimer = setInterval(refresh, 1000);
    select('dictations');
  }
  init();
  window.addEventListener('beforeunload', () => clearInterval(refreshTimer));
})();
