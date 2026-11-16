"use strict";
(() => {
  // Browser/PWA mode intentionally remains standalone and has no native API dependency.
  const native = window.__TAURI__;
  if (!native?.window?.getCurrentWindow || !native?.dpi?.LogicalSize) return;

  const appWindow = native.window.getCurrentWindow();
  const LogicalSize = native.dpi.LogicalSize;
  const PREF_KEY = "kneekura.prompt-wheel.desktop.v1";
  const defaults = {
    compact: true,
    pinned: true,
    sizes: {
      compact: { width: 430, height: 675 },
      expanded: { width: 1020, height: 760 }
    }
  };
  const $ = (id) => document.getElementById(id);
  const safeSize = (value, fallback) => {
    if (!value || typeof value !== "object") return { ...fallback };
    const width = Number(value.width), height = Number(value.height);
    if (!Number.isFinite(width) || !Number.isFinite(height)) return { ...fallback };
    return {
      width: Math.min(1900, Math.max(360, Math.round(width))),
      height: Math.min(1300, Math.max(460, Math.round(height)))
    };
  };

  function readPrefs() {
    try {
      const saved = JSON.parse(localStorage.getItem(PREF_KEY) || "{}");
      return {
        compact: saved.compact === undefined ? defaults.compact : saved.compact === true,
        pinned: saved.pinned === undefined ? defaults.pinned : saved.pinned === true,
        sizes: {
          compact: safeSize(saved.sizes?.compact, defaults.sizes.compact),
          expanded: safeSize(saved.sizes?.expanded, defaults.sizes.expanded)
        }
      };
    } catch (_) {
      return structuredClone(defaults);
    }
  }

  const prefs = readPrefs();
  let muteResizeUntil = 0;
  let resizeTimer;
  let updating = false;
  const persist = () => {
    try { localStorage.setItem(PREF_KEY, JSON.stringify(prefs)); }
    catch (_) { /* Unavailable storage should not block native controls. */ }
  };
  function render() {
    document.body.classList.add("desktop-mode");
    document.body.classList.toggle("compact-mode", prefs.compact);
    $("pinToggle").textContent = prefs.pinned ? "● 固定中" : "○ 固定OFF";
    $("pinToggle").setAttribute("aria-pressed", String(prefs.pinned));
    $("pinToggle").title = prefs.pinned ? "最前面固定を解除" : "常に最前面に固定";
    $("modeToggle").textContent = prefs.compact ? "▣ 一覧" : "◉ 小型";
    $("modeToggle").title = prefs.compact ? "プロンプトの一覧を広げる" : "小型ホイールに戻す";
    $("hideWindow").title = "タスクトレイに隠す";
    $("desktopControls").hidden = false;
  }

  async function resizeToMode() {
    const size = prefs.sizes[prefs.compact ? "compact" : "expanded"];
    // DOM resize events emitted by setSize must not overwrite the user's saved size.
    muteResizeUntil = Date.now() + 1100;
    await appWindow.setSize(new LogicalSize(size.width, size.height));
  }

  async function togglePin() {
    if (updating) return;
    updating = true;
    const next = !prefs.pinned;
    try {
      await appWindow.setAlwaysOnTop(next);
      prefs.pinned = next;
      persist(); render();
    } catch (error) {
      console.warn("Unable to change always-on-top:", error);
      window.alert("最前面表示の切り替えに失敗しました。");
    } finally { updating = false; }
  }

  async function toggleMode() {
    if (updating) return;
    updating = true;
    const previous = prefs.compact;
    prefs.compact = !previous;
    render();
    try {
      await resizeToMode();
      persist();
    } catch (error) {
      prefs.compact = previous;
      render();
      console.warn("Unable to resize:", error);
      window.alert("表示サイズの切り替えに失敗しました。");
    } finally { updating = false; }
  }

  $("pinToggle").addEventListener("click", togglePin);
  $("modeToggle").addEventListener("click", toggleMode);
  $("hideWindow").addEventListener("click", async () => {
    try { await appWindow.hide(); }
    catch (error) { console.warn("Unable to hide:", error); }
  });

  // Window size changes through the normal OS border are remembered per mode.
  window.addEventListener("resize", () => {
    if (Date.now() < muteResizeUntil || updating) return;
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => {
      if (Date.now() < muteResizeUntil) return;
      const mode = prefs.compact ? "compact" : "expanded";
      prefs.sizes[mode] = safeSize(
        { width: window.innerWidth, height: window.innerHeight },
        prefs.sizes[mode]
      );
      persist();
    }, 350);
  });

  // Native close -> tray is implemented on the Rust side. The tray's exit menu
  // actually terminates the application, preventing the hide/exit ambiguity.
  render();
  void (async () => {
    try {
      await appWindow.setAlwaysOnTop(prefs.pinned);
      await resizeToMode();
    } catch (error) { console.warn("Unable to restore desktop preferences:", error); }
  })();
})();
