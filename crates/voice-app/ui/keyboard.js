// Talk-key picker drawn as a whole keyboard (Settings and onboarding step 3).
// Every key is shown so the choice reads in place; only the keys in
// `get_settings().hotkeys` (Hotkey::available on the Rust side) are
// selectable, the rest are dimmed. Clicking a talk key, or pressing it while
// the picker is on screen, calls `onSelect(id)`. The fn/🌐 key never reaches
// a webview, so it can only be clicked.
(() => {
  // [label, width in key units, options]. Each row adds up to 15 units so the
  // rows line up; `id` is a `Hotkey::raw_value`, `code` a KeyboardEvent.code.
  const F_ROW = (esc) => [[esc, 1.5, { code: 'Escape' }],
    ...Array.from({ length: 12 }, (_, i) => ['F' + (i + 1), 1.125, { code: 'F' + (i + 1) }])];
  const letters = (s) => [...s].map((c) => [c, 1, { code: /[A-Z]/.test(c) ? 'Key' + c : null }]);
  const digits = [...'1234567890'].map((c) => [c, 1, { code: 'Digit' + c }]);
  const MIDDLE = (bksp, tab, caps, enter, shift) => [
    [['`', 1, { code: 'Backquote' }], ...digits, ['-', 1, { code: 'Minus' }],
      ['=', 1, { code: 'Equal' }], [bksp, 2, { code: 'Backspace' }]],
    [[tab, 1.5, { code: 'Tab' }], ...letters('QWERTYUIOP'), ['[', 1, { code: 'BracketLeft' }],
      [']', 1, { code: 'BracketRight' }], ['\\', 1.5, { code: 'Backslash' }]],
    [[caps, 1.75, { code: 'CapsLock' }], ...letters('ASDFGHJKL'), [';', 1, { code: 'Semicolon' }],
      ["'", 1, { code: 'Quote' }], [enter, 2.25, { code: 'Enter' }]],
    [[shift, 2.25, { code: 'ShiftLeft' }], ...letters('ZXCVBNM'), [',', 1, { code: 'Comma' }],
      ['.', 1, { code: 'Period' }], ['/', 1, { code: 'Slash' }], [shift, 2.75, { code: 'ShiftRight' }]],
  ];
  const ARROWS = ['arrows', 3, { arrows: true }];

  const LAYOUTS = {
    macos: [
      F_ROW('esc'),
      ...MIDDLE('delete', 'tab', 'caps lock', 'return', 'shift'),
      [['fn', 1, { id: 'fn', glyph: '🌐' }], ['control', 1.25, { glyph: '⌃', code: 'ControlLeft' }],
        ['option', 1.25, { glyph: '⌥', code: 'AltLeft' }], ['command', 1.5, { glyph: '⌘', code: 'MetaLeft' }],
        ['', 4.25, { code: 'Space' }],
        ['command', 1.5, { glyph: '⌘', code: 'MetaRight', id: 'rightCommand' }],
        ['option', 1.25, { glyph: '⌥', code: 'AltRight', id: 'rightOption' }], ARROWS],
    ],
    pc: (win) => [
      F_ROW('Esc'),
      ...MIDDLE('Backspace', 'Tab', 'Caps', 'Enter', 'Shift'),
      [['Ctrl', 1.25, { code: 'ControlLeft' }], [win, 1, { code: 'MetaLeft' }],
        ['Alt', 1.25, { code: 'AltLeft' }], ['', 5, { code: 'Space' }],
        ['Alt', 1.25, { code: 'AltRight', id: 'rightOption' }],
        [win, 1, { code: 'MetaRight', id: 'rightCommand' }],
        ['Ctrl', 1.25, { code: 'ControlRight', id: 'rightControl' }], ARROWS],
    ],
  };
  // Older WebKitGTK still reports the Super keys as OSLeft/OSRight.
  const CODE_ALIASES = { OSLeft: 'MetaLeft', OSRight: 'MetaRight' };

  function rowsFor(platform) {
    if (platform === 'macos') return LAYOUTS.macos;
    return LAYOUTS.pc(platform === 'windows' ? 'Win' : 'Super');
  }

  function keyEl(label, width, opts) {
    const k = document.createElement('div');
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

  // `opts`: { platform, hotkeys: [{id,label,short_label}], selected, onSelect }.
  // Returns { update(selected) } so a re-render keeps focus on the keys.
  function render(root, opts) {
    const available = new Map(opts.hotkeys.map((hk) => [hk.id, hk]));
    const byId = new Map();
    const byCode = new Map();
    root.textContent = '';
    root.classList.add('kb');
    root.setAttribute('role', 'radiogroup');
    root.setAttribute('aria-label', 'Talk key');

    rowsFor(opts.platform).forEach((row, i) => {
      const r = document.createElement('div');
      r.className = 'kb-row' + (i === 0 ? ' fn-row' : '');
      for (const [label, width, o] of row) {
        if (o.arrows) {
          const a = document.createElement('div');
          a.className = 'kb-arrows';
          a.style.setProperty('--w', width);
          a.setAttribute('aria-hidden', 'true');
          const up = keyEl('▲', 1, {});
          const down = keyEl('▼', 1, {});
          const stack = document.createElement('div');
          stack.className = 'kb-updown';
          stack.append(up, down);
          a.append(keyEl('◀', 1, {}), stack, keyEl('▶', 1, {}));
          for (const [el, code] of [[a.children[0], 'ArrowLeft'], [up, 'ArrowUp'],
            [down, 'ArrowDown'], [a.children[2], 'ArrowRight']]) byCode.set(code, el);
          r.appendChild(a);
          continue;
        }
        const hk = o.id && available.get(o.id);
        let k;
        if (hk) {
          // A real control: the selectable talk keys are buttons in a radio group.
          k = document.createElement('button');
          k.type = 'button';
          const inner = keyEl(label, width, o);
          k.className = inner.className + ' talk';
          k.style.setProperty('--w', width);
          k.append(...inner.childNodes);
          k.setAttribute('role', 'radio');
          k.setAttribute('aria-label', hk.label);
          k.title = 'Hold ' + hk.label + ' to talk';
          k.dataset.id = hk.id;
          k.addEventListener('click', () => opts.onSelect(hk.id));
          byId.set(hk.id, k);
        } else {
          k = keyEl(label, width, o);
          k.classList.add('off');
          k.setAttribute('aria-hidden', 'true');
        }
        if (o.code) byCode.set(o.code, k);
        r.appendChild(k);
      }
      root.appendChild(r);
    });

    function update(selected) {
      for (const [id, k] of byId) {
        const on = id === selected;
        k.classList.toggle('sel', on);
        k.setAttribute('aria-checked', on ? 'true' : 'false');
      }
    }
    update(opts.selected);

    // Pressing a key while the picker is showing lights it up, and pressing
    // one of the talk keys picks it. Text fields elsewhere keep their keys.
    function onKey(e) {
      if (!root.isConnected || root.offsetParent === null) return;
      const t = e.target;
      if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
      const code = CODE_ALIASES[e.code] || e.code;
      const k = byCode.get(code);
      if (!k) return;
      if (e.type === 'keyup') { k.classList.remove('down'); return; }
      if (e.repeat) return;
      k.classList.add('down');
      if (k.dataset.id && available.has(k.dataset.id)) opts.onSelect(k.dataset.id);
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
