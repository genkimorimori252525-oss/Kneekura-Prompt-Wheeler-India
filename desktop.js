"use strict";
// Native bridge for the Tauri overlay window. In a plain browser this only tags <html class="web">.
(() => {
  const tauri = window.__TAURI__;
  const api = tauri?.window, dpi = tauri?.dpi;
  if (!api?.getCurrentWindow || !dpi?.PhysicalSize || !dpi?.PhysicalPosition) {
    document.documentElement.classList.add("web");
    return;
  }
  document.documentElement.classList.add("desktop");

  const win = api.getCurrentWindow();
  const PREF_KEY = "kneekura.prompt-wheel.desktop.v2";
  // Logical sizes at 100% scale; must match styles.css.
  const SIZES = { mini: [60, 60], ring: [320, 320], drawer: [320, 610] };

  function readPrefs() {
    try {
      const saved = JSON.parse(localStorage.getItem(PREF_KEY) || "{}") || {};
      const pos = saved.pos && Number.isFinite(saved.pos.x) && Number.isFinite(saved.pos.y) ? { x: saved.pos.x, y: saved.pos.y } : null;
      return { pinned: saved.pinned !== false, pos };
    } catch (_) { return { pinned: true, pos: null }; }
  }
  const prefs = readPrefs();
  const persist = () => {
    try { localStorage.setItem(PREF_KEY, JSON.stringify(prefs)); } catch (_) { /* keep working without storage */ }
  };

  async function workArea() {
    const monitor = (await api.currentMonitor()) || (await api.primaryMonitor());
    if (!monitor) return null;
    const area = monitor.workArea || monitor;
    return { x: area.position.x, y: area.position.y, w: area.size.width, h: area.size.height };
  }

  // Resize for a view while pinning the window corner nearest to a screen corner,
  // so a widget parked bottom-right grows up/left instead of off-screen.
  async function layout(view, zoom = 1) {
    const [logicalW, logicalH] = SIZES[view] || SIZES.ring;
    const scale = (await win.scaleFactor()) * zoom;
    const pos = await win.outerPosition(), size = await win.outerSize();
    const area = await workArea();
    const w = Math.round(logicalW * scale), h = Math.round(logicalH * scale);
    let right = false, bottom = false, x = pos.x, y = pos.y;
    if (area) {
      right = pos.x + size.width / 2 > area.x + area.w / 2;
      bottom = pos.y + size.height / 2 > area.y + area.h / 2;
    }
    if (right) x = pos.x + size.width - w;
    if (bottom) y = pos.y + size.height - h;
    if (area) {
      x = Math.min(Math.max(x, area.x), area.x + area.w - w);
      y = Math.min(Math.max(y, area.y), area.y + area.h - h);
    }
    await win.setSize(new dpi.PhysicalSize(w, h));
    await win.setPosition(new dpi.PhysicalPosition(Math.round(x), Math.round(y)));
    return { right, bottom };
  }

  let moveTimer, lastMoved = 0;
  void win.onMoved(({ payload }) => {
    lastMoved = Date.now();
    clearTimeout(moveTimer);
    moveTimer = setTimeout(() => { prefs.pos = { x: payload.x, y: payload.y }; persist(); }, 400);
  });

  // Frameless window: drag it from anywhere that is not a text field or the scrolling list.
  // A press that does not travel stays an ordinary click.
  let pressed = null;
  document.addEventListener("mousedown", (event) => {
    pressed = event.button === 0 && !event.target.closest("input, textarea, select, .list") ? { x: event.screenX, y: event.screenY } : null;
  });
  document.addEventListener("mousemove", (event) => {
    if (!pressed) return;
    if (!(event.buttons & 1)) { pressed = null; return; }
    if (Math.hypot(event.screenX - pressed.x, event.screenY - pressed.y) > 5) {
      pressed = null;
      lastMoved = Date.now();
      void win.startDragging();
    }
  });
  // If the webview still reports a click at the end of a drag, drop it.
  document.addEventListener("click", (event) => {
    if (Date.now() - lastMoved < 250) { event.stopPropagation(); event.preventDefault(); }
  }, true);

  window.PWHost = {
    get pinned() { return prefs.pinned; },
    layout,
    async togglePin() {
      const next = !prefs.pinned;
      try {
        await win.setAlwaysOnTop(next);
        prefs.pinned = next;
        persist();
      } catch (error) { console.warn("always-on-top change failed:", error); }
      return prefs.pinned;
    },
    // Called once at startup. The window is created hidden, so it never flashes at the wrong size.
    async ready(view, zoom) {
      let anchor = { right: false, bottom: false };
      try {
        await win.setAlwaysOnTop(prefs.pinned);
        if (prefs.pos) await win.setPosition(new dpi.PhysicalPosition(prefs.pos.x, prefs.pos.y));
        anchor = await layout(view, zoom);
      } finally {
        await win.show();
      }
      return anchor;
    }
  };
})();
