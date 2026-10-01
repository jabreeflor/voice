// Settings → Appearance for the main and onboarding windows: mirrors the
// stored choice onto <html data-theme>, which app.css keys its dark tokens
// on. "system" removes the attribute so `prefers-color-scheme` decides.
// The backend also sets the native window theme (commands::apply_appearance);
// the attribute is what makes a pinned choice hold on WebKitGTK, which does
// not always follow it.
(() => {
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  function apply(id) {
    const root = document.documentElement;
    if (id === 'light' || id === 'dark') root.dataset.theme = id;
    else delete root.dataset.theme;
  }
  listen('appearance-changed', (e) => apply(e.payload));
  invoke('get_settings').then((s) => apply(s.appearance)).catch(console.error);
})();
