// Talk-key picker drawn as a whole keyboard (Settings and onboarding step 3).
// Every key is drawn so the choice reads in place. The keys listed in
// `get_settings().hotkeys` (Hotkey::available on the Rust side) are selectable
// and tinted; the rest are dimmed. Clicking a selectable key, or pressing a
// selectable modifier or F-key while the picker is on screen, calls
// `onSelect(id)` with its raw value. Keys
// are matched by KeyboardEvent.code, which is what the Rust side also reports
// as `code`. fn/🌐 never reaches a webview, so it is matched by id and can only
// be clicked.
(() => {
  // A key is [label, width in key units, options]. Each row adds up to 15
  // units so the rows line up. `code` is a KeyboardEvent.code; `id` (a raw
  // value) is used only for fn, the one key that has no code. The codes are
  // spelled out so each one can be found in this file.
  const F_ROW = (esc) => [
    [esc, 1.5, { code: 'Escape' }],
    ['F1', 1.125, { code: 'F1' }], ['F2', 1.125, { code: 'F2' }], ['F3', 1.125, { code: 'F3' }],
    ['F4', 1.125, { code: 'F4' }], ['F5', 1.125, { code: 'F5' }], ['F6', 1.125, { code: 'F6' }],
    ['F7', 1.125, { code: 'F7' }], ['F8', 1.125, { code: 'F8' }], ['F9', 1.125, { code: 'F9' }],
    ['F10', 1.125, { code: 'F10' }], ['F11', 1.125, { code: 'F11' }], ['F12', 1.125, { code: 'F12' }],
  ];
  const MIDDLE = (bksp, tab, caps, enter, shift) => [
    [['`', 1, { code: 'Backquote' }],
      ['1', 1, { code: 'Digit1' }], ['2', 1, { code: 'Digit2' }], ['3', 1, { code: 'Digit3' }],
      ['4', 1, { code: 'Digit4' }], ['5', 1, { code: 'Digit5' }], ['6', 1, { code: 'Digit6' }],
      ['7', 1, { code: 'Digit7' }], ['8', 1, { code: 'Digit8' }], ['9', 1, { code: 'Digit9' }],
      ['0', 1, { code: 'Digit0' }],
      ['-', 1, { code: 'Minus' }], ['=', 1, { code: 'Equal' }], [bksp, 2, { code: 'Backspace' }]],
    [[tab, 1.5, { code: 'Tab' }],
      ['Q', 1, { code: 'KeyQ' }], ['W', 1, { code: 'KeyW' }], ['E', 1, { code: 'KeyE' }],
      ['R', 1, { code: 'KeyR' }], ['T', 1, { code: 'KeyT' }], ['Y', 1, { code: 'KeyY' }],
      ['U', 1, { code: 'KeyU' }], ['I', 1, { code: 'KeyI' }], ['O', 1, { code: 'KeyO' }],
      ['P', 1, { code: 'KeyP' }],
      ['[', 1, { code: 'BracketLeft' }], [']', 1, { code: 'BracketRight' }],
      ['\\', 1.5, { code: 'Backslash' }]],
    [[caps, 1.75, { code: 'CapsLock' }],
      ['A', 1, { code: 'KeyA' }], ['S', 1, { code: 'KeyS' }], ['D', 1, { code: 'KeyD' }],
      ['F', 1, { code: 'KeyF' }], ['G', 1, { code: 'KeyG' }], ['H', 1, { code: 'KeyH' }],
      ['J', 1, { code: 'KeyJ' }], ['K', 1, { code: 'KeyK' }], ['L', 1, { code: 'KeyL' }],
      [';', 1, { code: 'Semicolon' }], ["'", 1, { code: 'Quote' }],
      [enter, 2.25, { code: 'Enter' }]],
    [[shift, 2.25, { code: 'ShiftLeft' }],
      ['Z', 1, { code: 'KeyZ' }], ['X', 1, { code: 'KeyX' }], ['C', 1, { code: 'KeyC' }],
      ['V', 1, { code: 'KeyV' }], ['B', 1, { code: 'KeyB' }], ['N', 1, { code: 'KeyN' }],
      ['M', 1, { code: 'KeyM' }],
      [',', 1, { code: 'Comma' }], ['.', 1, { code: 'Period' }], ['/', 1, { code: 'Slash' }],
      [shift, 2.75, { code: 'ShiftRight' }]],
  ];
  const ARROWS = ['arrows', 3, { arrows: true }];

  const LAYOUTS = {
    macos: [
      F_ROW('esc'),
      ...MIDDLE('delete', 'tab', 'caps lock', 'return', 'shift'),
      [['fn', 1, { id: 'fn', glyph: '🌐' }], ['control', 1.25, { glyph: '⌃', code: 'ControlLeft' }],
        ['option', 1.25, { glyph: '⌥', code: 'AltLeft' }], ['command', 1.5, { glyph: '⌘', code: 'MetaLeft' }],
        ['', 4.25, { code: 'Space' }],
        ['command', 1.5, { glyph: '⌘', code: 'MetaRight' }],
        ['option', 1.25, { glyph: '⌥', code: 'AltRight' }], ARROWS],
    ],
    pc: (win) => [
      F_ROW('Esc'),
      ...MIDDLE('Backspace', 'Tab', 'Caps', 'Enter', 'Shift'),
      [['Ctrl', 1.25, { code: 'ControlLeft' }], [win, 1, { code: 'MetaLeft' }],
        ['Alt', 1.25, { code: 'AltLeft' }], ['', 5, { code: 'Space' }],
        ['Alt', 1.25, { code: 'AltRight' }],
        [win, 1, { code: 'MetaRight' }],
        ['Ctrl', 1.25, { code: 'ControlRight' }], ARROWS],
    ],
  };
  // Older WebKitGTK still reports the Super keys as OSLeft/OSRight.
  const CODE_ALIASES = { OSLeft: 'MetaLeft', OSRight: 'MetaRight' };

  function rowsFor(platform) {
    if (platform === 'macos') return LAYOUTS.macos;
    return LAYOUTS.pc(platform === 'windows' ? 'Win' : 'Super');
  }

  // tag is 'button' for a selectable talk key, 'div' otherwise.
  function keyEl(label, width, opts, tag) {
    const k = document.createElement(tag);
    if (tag === 'button') k.type = 'button';
    k.className = 'kb-key';
    k.style.setProperty('--w', width);
    if (opts.glyph) {
      const g = document.createElement('b');
      g.textContent = opts.glyph;
      k.appendChild(g);
      const s = document.createElement('small');
      s.textContent = label;
      k.appendChild(s);
    } else {
      k.textContent = label;
    }
    if (width > 1.2 && !opts.glyph) k.classList.add('wide');
    else if (!opts.glyph && label.length > 2) k.classList.add('word');
    return k;
  }

  // One layout key. A key the platform offers is a radio button in the
  // group; any other key is dimmed and takes no clicks. Every coded key is
  // registered by code so a press can light it up.
  function makeKey(label, width, o, ctx) {
    const hk = ctx.offered.get(o.code || o.id);
    const k = keyEl(label, width, o, hk ? 'button' : 'div');
    if (hk) {
      k.classList.add('talk');
      k.setAttribute('role', 'radio');
      k.setAttribute('aria-label', hk.label);
      k.title = 'Hold ' + hk.label + ' to talk';
      k.dataset.id = hk.id;
      if (hk.types_text) k.dataset.text = '1';
      k.addEventListener('click', () => ctx.onSelect(hk.id));
      ctx.byId.set(hk.id, k);
    } else {
      k.classList.add('off');
      k.setAttribute('aria-hidden', 'true');
    }
    if (o.code) ctx.byCode.set(o.code, k);
    return k;
  }

  // The arrow keys: ◀ and ▶ flank a ▲ over ▼ stack.
  function arrowCluster(width, ctx) {
    const a = document.createElement('div');
    a.className = 'kb-arrows';
    a.style.setProperty('--w', width);
    const stack = document.createElement('div');
    stack.className = 'kb-updown';
    stack.append(makeKey('▲', 1, { code: 'ArrowUp' }, ctx), makeKey('▼', 1, { code: 'ArrowDown' }, ctx));
    a.append(makeKey('◀', 1, { code: 'ArrowLeft' }, ctx), stack, makeKey('▶', 1, { code: 'ArrowRight' }, ctx));
    return a;
  }

  // `opts`: { platform, hotkeys: [{id,label,short_label,code}], selected, onSelect }.
  // Returns { update(selected) } so a re-render keeps focus on the keys.
  function render(root, opts) {
    const ctx = {
      offered: new Map(opts.hotkeys.map((hk) => [hk.code || hk.id, hk])),
      byId: new Map(),
      byCode: new Map(),
      onSelect: opts.onSelect,
    };
    root.textContent = '';
    root.classList.add('kb');
    root.setAttribute('role', 'radiogroup');
    root.setAttribute('aria-label', 'Talk key');

    rowsFor(opts.platform).forEach((row, i) => {
      const r = document.createElement('div');
      r.className = 'kb-row' + (i === 0 ? ' fn-row' : '');
      for (const [label, width, o] of row) {
        r.appendChild(o.arrows ? arrowCluster(width, ctx) : makeKey(label, width, o, ctx));
      }
      root.appendChild(r);
    });
    const { byId, byCode } = ctx;

    function update(selected) {
      for (const [id, k] of byId) {
        const on = id === selected;
        k.classList.toggle('sel', on);
        k.setAttribute('aria-checked', on ? 'true' : 'false');
      }
    }
    update(opts.selected);

    // Pressing a key while the picker is showing lights it up, and pressing
    // a selectable modifier or F-key picks it. Keys that type are picked by
    // click only: Tab, Enter or Space pressed to move around the window would
    // otherwise become the talk key and stop working in every app. Text
    // fields elsewhere keep their keys.
    function onKey(e) {
      if (!root.isConnected || root.offsetParent === null) return;
      const t = e.target;
      if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
      const code = CODE_ALIASES[e.code] || e.code;
      const k = byCode.get(code);
      if (!k) return;
      const pick = k.dataset.id && byId.has(k.dataset.id) && !k.dataset.text ? k.dataset.id : null;
      // F5 would otherwise reload the webview.
      if (pick) e.preventDefault();
      if (e.type === 'keyup') { k.classList.remove('down'); return; }
      if (e.repeat) return;
      k.classList.add('down');
      if (pick) opts.onSelect(pick);
    }
    // A key released while the window is in the background never sends keyup.
    function onBlur() {
      root.querySelectorAll('.kb-key.down').forEach((k) => k.classList.remove('down'));
    }
    if (root._kbListeners) root._kbListeners();
    document.addEventListener('keydown', onKey);
    document.addEventListener('keyup', onKey);
    window.addEventListener('blur', onBlur);
    root._kbListeners = () => {
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('keyup', onKey);
      window.removeEventListener('blur', onBlur);
    };

    return { update };
  }

  window.VoiceKeyboard = { render };
})();
