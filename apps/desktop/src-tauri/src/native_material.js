(() => {
  if (window.__nativeHost) return;
  let current = { backdrop: 'none' };
  let observer;
  const apply = (state) => {
    current = state;
    const root = document.documentElement;
    if (!root) {
      if (!observer) {
        observer = new MutationObserver(() => {
          if (document.documentElement) {
            observer.disconnect();
            observer = undefined;
            apply(current);
          }
        });
        observer.observe(document, { childList: true });
      }
      return;
    }
    root.setAttribute('backdrop', state.backdrop);
    root.dataset.backdrop = state.backdrop;
    if (state.macos) root.setAttribute('macos', state.macos);
    if (typeof state.dark === 'boolean') root.toggleAttribute('dark', state.dark);
    if (state.accent) {
      root.style.setProperty('--color-system-accent-raw', state.accent);
      // The accent preference lives in localStorage (spacelens.appearance,
      // accent: 'system' | 'default'). 'default' opts out of the OS accent
      // so every surface uses the app's built-in colors.
      let accentPreference = 'system';
      try {
        accentPreference = JSON.parse(localStorage.getItem('spacelens.appearance') ?? '{}').accent ?? 'system';
      } catch {}
      if (accentPreference === 'system') applyAccent(root, state.accent);
      else clearAccent(root);
    }
    window.dispatchEvent(new CustomEvent('nativehost', { detail: state }));
  };
  const contrastOn = (hex) => {
    const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
    if (!m) return '#ffffff';
    const n = parseInt(m[1], 16);
    const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((c) => {
      const s = c / 255;
      return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * b > 0.45 ? '#1c1c1e' : '#ffffff';
  };
  const applyAccent = (root, hex) => {
    // The accent replaces --primary, so its paired foreground must be
    // derived from the accent itself — the theme's --primary-foreground
    // assumes shadcn's near-white dark primary and renders dark-on-blue.
    root.style.setProperty('--color-system-accent', hex);
    root.style.setProperty('--color-system-accent-foreground', contrastOn(hex));
  };
  const clearAccent = (root) => {
    root.style.removeProperty('--color-system-accent');
    root.style.removeProperty('--color-system-accent-foreground');
  };
  window.__nativeHost = { apply, get state() { return current; } };
  apply(current);
})();
