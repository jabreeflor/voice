// Floating pill: draws the level bars / processing wave and flashes messages.
// Mirrors BarsView + Overlay in core.swift. The Rust side owns show/hide of
// the window itself; this script only reacts to the `overlay`, `level` and
// `sound` events documented in src/overlay.rs.
(() => {
  const { listen } = window.__TAURI__.event;
  const pill = document.getElementById('pill');
  const label = document.getElementById('label');
  const canvas = document.getElementById('bars');
  const ctx = canvas.getContext('2d');
  const scale = 2; // canvas is drawn at 2x for crisp bars on HiDPI screens

  const BAR_COUNT = 17;
  const BAR_W = 3.5;
  const GAP = 4.5;
  let mode = 'hidden';
  let history = new Array(BAR_COUNT).fill(0);
  let phase = 0;
  let timer = null;

  function draw() {
    const w = canvas.width / scale;
    const h = canvas.height / scale;
    ctx.setTransform(scale, 0, 0, scale, 0, 0);
    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = 'rgba(255,255,255,0.92)';
    const totalW = BAR_COUNT * BAR_W + (BAR_COUNT - 1) * GAP;
    let x = (w - totalW) / 2;
    const midY = h / 2;
    for (let i = 0; i < BAR_COUNT; i++) {
      let bh = 3;
      if (mode === 'listening') bh = 3 + Math.min(1, history[i]) * (h - 14);
      else if (mode === 'processing') bh = 4 + (Math.sin(phase + i * 0.55) * 0.5 + 0.5) * 13;
      roundRect(x, midY - bh / 2, BAR_W, bh, BAR_W / 2);
      x += BAR_W + GAP;
    }
  }

  function roundRect(x, y, w, h, r) {
    ctx.beginPath();
    ctx.moveTo(x + r, y);
    ctx.arcTo(x + w, y, x + w, y + h, r);
    ctx.arcTo(x + w, y + h, x, y + h, r);
    ctx.arcTo(x, y + h, x, y, r);
    ctx.arcTo(x, y, x + w, y, r);
    ctx.closePath();
    ctx.fill();
  }

  // 30 Hz like the AppKit timer. In processing mode the wave advances by
  // itself; in listening mode the bars only move when a `level` arrives.
  function startTicking() {
    stopTicking();
    history = new Array(BAR_COUNT).fill(0);
    phase = 0;
    timer = setInterval(() => {
      if (mode === 'processing') phase += 0.28;
      draw();
    }, 1000 / 30);
  }
  function stopTicking() {
    if (timer) clearInterval(timer);
    timer = null;
  }

  listen('level', (e) => {
    if (mode !== 'listening') return;
    history.shift();
    history.push(Number(e.payload.value) || 0);
  });

  listen('overlay', (e) => {
    const { mode: next, message } = e.payload;
    mode = next;
    if (mode === 'hidden') {
      // Keep the current face (text or bars) while the pill fades out; the
      // `flash` class is swapped by the next show, as Overlay.hide() only
      // animated alpha and left label/bars visibility alone.
      stopTicking();
      pill.classList.remove('show');
      return;
    }
    pill.classList.toggle('flash', mode === 'flash');
    if (mode === 'flash') {
      stopTicking();
      label.textContent = message || '';
    } else {
      startTicking();
    }
    pill.classList.add('show');
  });

  listen('sound', (e) => {
    const el = document.getElementById('snd-' + e.payload.name);
    if (!el) return;
    el.currentTime = 0;
    el.play().catch(() => {});
  });

  draw();
})();
