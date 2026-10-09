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
    if (state.accent) root.style.setProperty('--color-system-accent', state.accent);
    window.dispatchEvent(new CustomEvent('nativehost', { detail: state }));
  };
  window.__nativeHost = { apply, get state() { return current; } };
  apply(current);
})();
