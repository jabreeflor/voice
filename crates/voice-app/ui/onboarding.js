// Onboarding window: five steps, mirrors OnboardingWindow in ui.swift.
(() => {
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const { getCurrentWindow } = window.__TAURI__.window;
  const $ = (id) => document.getElementById(id);

  let step = 1;
  let settings = null;
  let pollTimer = null;

  function goTo(n) {
    step = n;
    document.querySelectorAll('.ob-step').forEach((s) =>
      s.classList.toggle('visible', Number(s.dataset.step) === n));
    $('stepno').textContent = '0' + n + ' / 05';
    $('progress').style.width = (n * 20) + '%';
    $('back').classList.toggle('visible', n > 1);
    if (n === 2) poll();
    if (n === 4) $('try-text').focus();
    if (n === 5) renderDone();
  }
  document.querySelectorAll('[data-go]').forEach((b) =>
    b.addEventListener('click', () => goTo(Number(b.dataset.go))));
  $('back').addEventListener('click', () => { if (step > 1) goTo(step - 1); });

  // ── step 2: permissions ───────────────────────────────────────────────
  $('mic-btn').addEventListener('click', () => invoke('request_mic'));
  $('ax-btn').addEventListener('click', async () => {
    await invoke('request_accessibility');
    await invoke('open_accessibility_settings');
  });

  function markGranted(btn) {
    btn.textContent = 'Granted';
    btn.classList.remove('lav');
    btn.classList.add('granted');
    btn.disabled = true;
  }

  // Polled every 0.8 s while step 2 is showing, like the AppKit window: the
  // OS grants land outside the app, so there is no event to wait for. The
  // Accessibility card is macOS-only (the hook needs no grant elsewhere);
  // the microphone card goes away where the OS has no per-app consent.
  async function poll() {
    if (step !== 2) return;
    const p = await invoke('permission_state');
    const micApplies = p.mic !== 'not_applicable';
    const axApplies = settings.platform === 'macos';
    $('perm-mic').classList.toggle('hidden', !micApplies);
    $('perm-ax').classList.toggle('hidden', !axApplies);
    $('perm-none').hidden = micApplies || axApplies;
    if (micApplies && p.mic === 'granted') markGranted($('mic-btn'));
    if (axApplies && p.hotkeys_running) markGranted($('ax-btn'));
    // "Not denied" is enough to continue: on Windows the consent store has
    // no entry for a desktop app until Settings is touched, so the status
    // stays `undetermined` and there is no prompt that could ever move it
    // to `granted`. The button still only turns green on a real Allow. On
    // macOS `undetermined` only lasts while the prompt is up, and the poll
    // re-runs every 0.8 s, so nothing is skipped there.
    const micOk = !micApplies || p.mic !== 'denied';
    const axOk = !axApplies || p.hotkeys_running;
    $('perm-next').disabled = !(micOk && axOk);
  }

  // ── step 3: talk key ──────────────────────────────────────────────────
  function renderHotkeys() {
    const row = $('hk-row');
    row.textContent = '';
    settings.hotkeys.forEach((hk, i) => {
      const selected = hk.id === settings.hotkey;
      const b = document.createElement('button');
      b.className = 'hk' + (selected ? ' sel' : '');
      b.setAttribute('role', 'radio');
      b.setAttribute('aria-checked', selected ? 'true' : 'false');
      b.textContent = hk.short_label;
      const small = document.createElement('small');
      small.textContent = i === 0 ? 'recommended' : 'alternative';
      b.appendChild(small);
      b.addEventListener('click', async () => {
        await invoke('set_hotkey', { id: hk.id });
        settings.hotkey = hk.id;
        renderHotkeys();
      });
      row.appendChild(b);
    });
  }

  // ── step 5: done ──────────────────────────────────────────────────────
  function renderDone() {
    const hk = settings.hotkeys.find((h) => h.id === settings.hotkey) || settings.hotkeys[0];
    const where = settings.platform === 'macos' ? 'menu bar' : 'system tray';
    $('done-text').textContent = `Voice waits in your ${where}. Hold ${hk.short_label} in any app to dictate, and come back here for your dictation history and snippets.`;
  }
  $('finish').addEventListener('click', () => invoke('finish_onboarding'));

  async function init() {
    settings = await invoke('get_settings');
    document.body.classList.add(settings.platform);
    if (settings.platform === 'macos') {
      $('intro-text').textContent = 'Voice turns speech into text in any app on your Mac. It runs entirely on this computer — no accounts, no subscriptions, and nothing you say ever leaves your machine.';
    } else if (settings.platform === 'windows') {
      // Desktop apps get no consent prompt on Windows; the button opens the
      // Microphone privacy page instead (platform/windows.rs).
      $('mic-btn').textContent = 'Open Settings';
    }
    renderHotkeys();
    // A dictation that landed while step 4 is showing unlocks Continue.
    listen('dictation-landed', () => { if (step === 4) $('try-next').disabled = false; });
    // The permission poll runs only while the window is showing, like the
    // AppKit timer started in show() and invalidated in windowWillClose.
    // Replaying the wizard ("Setup Assistant…") reloads this page from the
    // Rust side, so every show after the first starts at step 1 with fresh
    // state; the registration is awaited before the visibility query so a
    // show that lands between the two IPC round trips is not missed
    // (`listen` only resolves once the listener is registered).
    // Targeted at this window's label: `emit_to` also reaches `Any`-target
    // listeners, so a default `listen` would follow the main window too.
    await listen('window-visible', (e) => setPolling(Boolean(e.payload)),
      { target: getCurrentWindow().label });
    const visible = await getCurrentWindow().isVisible().catch(() => true);
    setPolling(visible);
    goTo(1);
  }
  function setPolling(on) {
    if (on && !pollTimer) {
      pollTimer = setInterval(poll, 800);
      poll();
    } else if (!on && pollTimer) {
      clearInterval(pollTimer);
      pollTimer = null;
    }
  }
  init();
  window.addEventListener('beforeunload', () => setPolling(false));
})();
