"use strict";
(() => {
  // desktop.js (loaded first) provides the native window bridge; in a browser it is absent.
  const host = window.PWHost || null;
  const FAVORITES_KEY = "kneekura.prompt-wheel.favorites.v1";
  const UI_KEY = "kneekura.prompt-wheel.ui.v2";
  const CUSTOM_KEY = "kneekura.prompt-wheel.custom.v1";
  const VIEW_RANK = { mini: 0, ring: 1, drawer: 2 };
  const SVG_NS = "http://www.w3.org/2000/svg";
  const R_OUT = 150, R_IN = 98, R_LABEL = 124, GAP_DEG = 1.6;

  const state = {
    categories: [], prompts: [], byCategory: new Map(),
    turn: 0, indices: Object.create(null),
    search: "", favoritesOnly: false, favorites: new Set(),
    view: "ring", opacity: 80, scale: 1, switching: false,
    base: [], custom: { edits: {}, added: [] }, editing: null,
    genreAccum: 0, genreAt: 0, promptAccum: 0, promptAt: 0
  };
  const $ = (id) => document.getElementById(id);
  const modulo = (n, m) => ((n % m) + m) % m;
  const make = (tag, className, text) => {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
  };
  const svg = (tag, attrs) => {
    const node = document.createElementNS(SVG_NS, tag);
    for (const key in attrs) node.setAttribute(key, attrs[key]);
    return node;
  };
  const sourceBadge = (type) => type === "user-supplied" ? "提供画像" : type === "adapted" ? "Reddit参考・再構成" : "オリジナル";

  const categoryIndex = () => modulo(state.turn, state.categories.length || 1);
  const currentCategory = () => state.categories[categoryIndex()];
  const promptsOf = (category) => (category && state.byCategory.get(category.id)) || [];
  function currentPrompt() {
    const category = currentCategory(), list = promptsOf(category);
    if (!list.length) return null;
    return list[modulo(state.indices[category.id] || 0, list.length)];
  }

  // ---------- storage ----------
  function readJson(key, fallback) {
    try { return JSON.parse(localStorage.getItem(key)) ?? fallback; } catch (_) { return fallback; }
  }
  function writeJson(key, value) {
    try { localStorage.setItem(key, JSON.stringify(value)); } catch (_) { /* storage may be disabled */ }
  }
  const saveUi = () => writeJson(UI_KEY, { view: state.view, opacity: state.opacity, scale: state.scale });

  // ---------- user edits ----------
  // Built-in prompts stay untouched in data/prompts.json; edits and additions are kept
  // beside them in local storage and layered on top, so an edit can always be reverted.
  function loadCustom() {
    const saved = readJson(CUSTOM_KEY, null);
    const text = (v) => typeof v === "string";
    const edits = {}, added = [];
    if (saved && typeof saved.edits === "object" && saved.edits) {
      for (const [id, e] of Object.entries(saved.edits)) {
        if (e && text(e.title) && text(e.summary) && text(e.prompt)) edits[id] = { title: e.title, summary: e.summary, prompt: e.prompt };
      }
    }
    if (saved && Array.isArray(saved.added)) {
      for (const p of saved.added) {
        if (p && text(p.id) && text(p.category) && text(p.title) && text(p.summary) && text(p.prompt)) {
          added.push({ id: p.id, category: p.category, title: p.title, summary: p.summary, prompt: p.prompt });
        }
      }
    }
    state.custom = { edits, added };
  }
  function rebuildPrompts() {
    const keepId = currentPrompt()?.id;
    const known = new Set(state.categories.map((c) => c.id));
    state.prompts = [
      ...state.base.map((p) => state.custom.edits[p.id] ? { ...p, ...state.custom.edits[p.id], origin: "edited" } : p),
      ...state.custom.added.filter((p) => known.has(p.category)).map((p) => ({ ...p, origin: "added", source: { label: "自作", type: "original" } }))
    ];
    state.byCategory = new Map(state.categories.map((c) => [c.id, state.prompts.filter((p) => p.category === c.id)]));
    const kept = keepId && state.prompts.find((p) => p.id === keepId);
    if (kept) state.indices[kept.category] = promptsOf(state.categories.find((c) => c.id === kept.category)).indexOf(kept);
  }
  function commitCustom(focusId) {
    writeJson(CUSTOM_KEY, state.custom);
    rebuildPrompts();
    state.editing = null;
    const target = focusId && state.prompts.find((p) => p.id === focusId);
    if (target) selectPrompt(target); else render();
  }

  // ---------- bubble ----------
  let bubbleTimer;
  function hideBubble() { clearTimeout(bubbleTimer); $("bubble").classList.remove("show"); }
  // Show the prompt text over the wheel. With a duration it fades by itself unless the pointer rests on the centre.
  function peek(duration) {
    if (state.view === "mini" || !currentPrompt()) return;
    clearTimeout(bubbleTimer);
    $("bubble").classList.add("show");
    if (duration) bubbleTimer = setTimeout(() => { if (!$("core").matches(":hover")) hideBubble(); }, duration);
  }

  // ---------- feedback ----------
  let flashTimer;
  function flash(message, ok = true) {
    const core = $("core");
    $("coreFlash").textContent = message;
    core.classList.toggle("bad", !ok);
    core.classList.add("flash");
    $("live").textContent = message;
    clearTimeout(flashTimer);
    flashTimer = setTimeout(() => core.classList.remove("flash"), 1100);
  }

  async function copyText(value) {
    try {
      if (navigator.clipboard && window.isSecureContext) {
        await navigator.clipboard.writeText(value);
        return true;
      }
    } catch (_) { /* fall through to the legacy path */ }
    const field = document.createElement("textarea");
    field.value = value;
    field.setAttribute("readonly", "");
    field.style.cssText = "position:fixed;left:-9999px;top:0;opacity:0";
    document.body.append(field);
    field.select();
    let copied = false;
    try { copied = document.execCommand("copy"); } catch (_) {}
    field.remove();
    return copied;
  }
  async function copyWithFlash(text, okMessage) {
    const ok = await copyText(text);
    hideBubble();
    flash(ok ? okMessage : "コピーできませんでした", ok);
  }
  function copyCurrent() {
    const picked = currentPrompt();
    return picked ? copyWithFlash(picked.prompt, "✓ コピーしました") : undefined;
  }

  // ---------- ring ----------
  const polar = (radius, deg) => {
    const rad = deg * Math.PI / 180;
    return (radius * Math.cos(rad)).toFixed(2) + " " + (radius * Math.sin(rad)).toFixed(2);
  };
  function sectorPath(from, to) {
    return "M" + polar(R_OUT, from) + "A" + R_OUT + " " + R_OUT + " 0 0 1 " + polar(R_OUT, to) +
      "L" + polar(R_IN, to) + "A" + R_IN + " " + R_IN + " 0 0 0 " + polar(R_IN, from) + "Z";
  }

  function buildRing() {
    const ring = $("ring"), n = state.categories.length, step = 360 / n;
    ring.replaceChildren();
    state.categories.forEach((category, i) => {
      const mid = -90 + i * step;
      const seg = svg("path", { class: "seg", d: sectorPath(mid - step / 2 + GAP_DEG / 2, mid + step / 2 - GAP_DEG / 2) });
      seg.dataset.index = String(i);
      seg.style.setProperty("--c", category.color);
      seg.append(Object.assign(svg("title", {}), { textContent: category.label }));
      seg.addEventListener("click", () => moveTo(i));

      const [x, y] = polar(R_LABEL, mid).split(" ");
      const label = svg("g", { class: "lab", transform: "translate(" + x + " " + y + ")" });
      label.dataset.index = String(i);
      const inner = svg("g", { class: "lab-in" });
      inner.append(
        Object.assign(svg("text", { class: "ico", y: "-1" }), { textContent: category.icon }),
        Object.assign(svg("text", { class: "nm", y: "14" }), { textContent: category.short })
      );
      label.append(inner);
      ring.append(seg, label);
    });
    $("puck").style.background = "conic-gradient(from " + (-step / 2) + "deg," +
      state.categories.map((c, i) => c.color + " " + i * step + "deg " + (i + 1) * step + "deg").join(",") + ")";
  }

  function render() {
    const n = state.categories.length;
    if (!n) return;
    const step = 360 / n, active = categoryIndex(), category = currentCategory();
    const list = promptsOf(category), picked = currentPrompt();
    const ring = $("ring");
    // The whole ring turns so the active genre sits at 12 o'clock; labels counter-rotate to stay upright.
    ring.style.transform = "rotate(" + (-state.turn * step) + "deg)";
    for (const node of ring.children) {
      node.classList.toggle("on", Number(node.dataset.index) === active);
      if (node.classList.contains("lab")) node.firstChild.style.transform = "rotate(" + (state.turn * step) + "deg)";
    }
    $("widget").style.setProperty("--c", category.color);
    $("coreGenre").textContent = category.short;
    $("coreCount").textContent = list.length ? (list.indexOf(picked) + 1) + "/" + list.length : "0";
    $("coreTitle").textContent = picked ? picked.title : "プロンプトなし";
    $("bubbleTitle").textContent = picked ? picked.title : "";
    $("bubbleBody").textContent = picked ? picked.prompt : "";
    $("core").setAttribute("aria-label", picked ? category.label + "、" + picked.title + "。クリックでコピー" : "プロンプトなし");
    $("puckIcon").textContent = category.icon;
    $("puck").title = picked ? category.short + "：" + picked.title + "（クリックで開く／ホイールで切替）" : "開く";
    const isFavorite = !!picked && state.favorites.has(picked.id);
    $("btnFav").setAttribute("aria-pressed", String(isFavorite));
    $("btnFav").setAttribute("aria-label", isFavorite ? "お気に入りから外す" : "お気に入りに追加");
    if (state.view === "drawer") renderList();
  }

  function spinGenre(direction) { hideBubble(); state.turn += direction; render(); }
  function moveTo(index) {
    hideBubble();
    const n = state.categories.length;
    let delta = modulo(index - categoryIndex(), n);
    if (delta > n / 2) delta -= n;
    state.turn += delta;
    render();
  }
  function spinPrompt(direction) {
    const category = currentCategory(), list = promptsOf(category);
    if (!list.length) return;
    state.indices[category.id] = modulo((state.indices[category.id] || 0) + direction, list.length);
    render();
    peek(2600);
  }
  function selectPrompt(prompt) {
    const index = state.categories.findIndex((c) => c.id === prompt.category);
    if (index < 0) return;
    state.indices[prompt.category] = promptsOf(state.categories[index]).indexOf(prompt);
    moveTo(index);
  }
  function toggleFavorite(prompt) {
    if (!prompt) return;
    if (state.favorites.has(prompt.id)) state.favorites.delete(prompt.id);
    else state.favorites.add(prompt.id);
    writeJson(FAVORITES_KEY, [...state.favorites]);
    render();
  }

  // ---------- drawer ----------
  function rowFor(prompt, selected) {
    const category = state.categories.find((c) => c.id === prompt.category);
    const row = make("div", "row" + (selected ? " on" : ""));
    row.setAttribute("role", "listitem");
    row.style.setProperty("--rc", category?.color || "#8395ff");
    const head = make("div", "row-head");
    const main = make("button", "row-main");
    main.type = "button";
    main.title = "クリックで選択してコピー";
    const text = make("span", "row-text");
    const title = make("span", "row-title", prompt.title);
    if (prompt.origin) title.append(make("span", "tag", prompt.origin === "added" ? "自作" : "編集済"));
    text.append(title, make("span", "row-sum", prompt.summary));
    main.append(make("span", "row-dot"), text);
    main.addEventListener("click", () => { selectPrompt(prompt); void copyCurrent(); });
    const star = make("button", "row-star", state.favorites.has(prompt.id) ? "★" : "☆");
    star.type = "button";
    star.setAttribute("aria-pressed", String(state.favorites.has(prompt.id)));
    star.setAttribute("aria-label", prompt.title + "のお気に入りを切り替え");
    star.addEventListener("click", () => toggleFavorite(prompt));
    head.append(main, star);
    row.append(head);
    if (!selected) return row;

    const body = make("div", "row-body");
    body.append(make("p", "", prompt.prompt));
    if (prompt.note) body.append(make("p", "row-note", "注意: " + prompt.note));
    const acts = make("div", "row-acts");
    const edit = make("button", "", "✎ 編集");
    edit.type = "button";
    edit.addEventListener("click", () => { state.editing = { id: prompt.id }; renderList(); });
    acts.append(edit);
    if (prompt.promptEn) {
      const en = make("button", "", "EN原文をコピー");
      en.type = "button";
      en.addEventListener("click", () => copyWithFlash(prompt.promptEn, "✓ 英語原文をコピー"));
      acts.append(en);
    }
    if (prompt.source?.url?.startsWith("https://")) {
      const link = make("a", "", sourceBadge(prompt.source.type) + " ↗");
      link.href = prompt.source.url;
      link.target = "_blank";
      link.rel = "noopener noreferrer";
      link.title = prompt.source.label;
      // The desktop webview has no browser to hand links to, so copy the address instead.
      if (host) link.addEventListener("click", (event) => { event.preventDefault(); void copyWithFlash(prompt.source.url, "✓ 出典URLをコピー"); });
      acts.append(link);
    } else {
      acts.append(make("span", "count", sourceBadge(prompt.source.type)));
    }
    body.append(acts);
    row.append(body);
    return row;
  }

  function editorFor(editing) {
    const existing = editing.id ? state.prompts.find((p) => p.id === editing.id) : null;
    const form = make("form", "editor");
    const title = make("input"); title.placeholder = "タイトル"; title.maxLength = 60; title.value = existing?.title || "";
    const summary = make("input"); summary.placeholder = "ひとこと説明（省略可）"; summary.maxLength = 80; summary.value = existing?.summary || "";
    const body = make("textarea"); body.placeholder = "プロンプト本文"; body.spellcheck = false; body.value = existing?.prompt || "";
    const genre = make("select");
    genre.setAttribute("aria-label", "ジャンル");
    for (const c of state.categories) genre.append(Object.assign(make("option", "", c.short), { value: c.id }));
    genre.value = existing?.category || currentCategory().id;
    // Built-in prompts keep their genre so the bundled catalog and the edits never disagree about where one lives.
    genre.disabled = !!existing && existing.origin !== "added";
    const top = make("div", "editor-row");
    top.append(genre, title);
    const message = make("span", "editor-msg");
    const acts = make("div", "editor-acts");
    const save = make("button", "primary", "保存"); save.type = "submit";
    const cancel = make("button", "", "やめる"); cancel.type = "button";
    cancel.addEventListener("click", () => { state.editing = null; renderList(); });
    acts.append(save, cancel);
    if (existing?.origin === "edited") {
      const revert = make("button", "danger", "元に戻す"); revert.type = "button";
      revert.addEventListener("click", () => { delete state.custom.edits[existing.id]; commitCustom(existing.id); });
      acts.append(revert);
    } else if (existing?.origin === "added") {
      const remove = make("button", "danger", "削除"); remove.type = "button";
      remove.addEventListener("click", () => {
        if (remove.dataset.armed !== "1") { remove.dataset.armed = "1"; remove.textContent = "本当に削除"; return; }
        state.custom.added = state.custom.added.filter((p) => p.id !== existing.id);
        state.favorites.delete(existing.id);
        writeJson(FAVORITES_KEY, [...state.favorites]);
        commitCustom(null);
      });
      acts.append(remove);
    }
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      const next = { title: title.value.trim(), prompt: body.value.trim() };
      if (!next.title || !next.prompt) { message.textContent = "タイトルと本文を入れてください。"; return; }
      next.summary = summary.value.trim() || next.prompt.replace(/\s+/g, " ").slice(0, 36);
      let id = existing?.id;
      if (!existing) {
        id = "user-" + Date.now().toString(36);
        state.custom.added.push({ id, category: genre.value, ...next });
      } else if (existing.origin === "added") {
        Object.assign(state.custom.added.find((p) => p.id === id), next, { category: genre.value });
      } else {
        state.custom.edits[id] = next;
      }
      commitCustom(id);
    });
    form.append(top, summary, body, message, acts);
    return form;
  }

  function renderList() {
    if (state.editing) {
      const form = editorFor(state.editing);
      $("list").replaceChildren(form);
      form.querySelector("input").focus({ preventScroll: true });
      return;
    }
    const needle = state.search.trim().toLocaleLowerCase("ja");
    const everywhere = needle.length > 0 || state.favoritesOnly;
    const category = currentCategory(), picked = currentPrompt();
    const visible = state.prompts.filter((p) => {
      if (!everywhere && p.category !== category.id) return false;
      if (state.favoritesOnly && !state.favorites.has(p.id)) return false;
      if (!needle) return true;
      const label = state.categories.find((c) => c.id === p.category)?.label || "";
      return [p.title, p.summary, p.prompt, p.promptEn || "", label, p.source.label].join(" ").toLocaleLowerCase("ja").includes(needle);
    });
    const list = $("list");
    const keepScroll = list.scrollTop;
    list.replaceChildren();
    if (!visible.length) {
      list.append(make("p", "empty", needle ? "一致するプロンプトがありません。" : state.favoritesOnly ? "お気に入りはまだありません。☆で追加できます。" : "このジャンルは未登録です。"));
    } else {
      for (const p of visible) list.append(rowFor(p, p === picked));
    }
    list.scrollTop = keepScroll;
    const selectedRow = list.querySelector(".row.on");
    if (selectedRow) selectedRow.scrollIntoView({ block: "nearest" });
    $("search").placeholder = "全" + state.prompts.length + "件から検索…";
  }

  // ---------- views ----------
  function placeCorners(anchor) {
    const right = !!anchor?.right, bottom = !!anchor?.bottom;
    document.body.dataset.ah = right ? "right" : "left";
    document.body.dataset.av = bottom ? "bottom" : "top";
    // The collapse button lives in the corner where the puck will appear, so one spot toggles both ways.
    const home = (bottom ? "b" : "t") + (right ? "r" : "l");
    const rest = ["tl", "tr", "br", "bl"].filter((c) => c !== home);
    $("btnMini").className = "corner c-" + home;
    ["btnPin", "btnList", "btnFav"].forEach((id, i) => { $(id).className = "corner c-" + rest[i]; });
  }

  function applyView(view) {
    state.view = view;
    if (view !== "drawer") state.editing = null;
    if (view === "mini") hideBubble();
    $("widget").dataset.view = view;
    $("widget").classList.toggle("awake", view === "drawer");
    $("btnList").setAttribute("aria-pressed", String(view === "drawer"));
    if (view === "drawer") renderList();
  }

  async function setView(next) {
    if (state.switching || next === state.view || !(next in VIEW_RANK)) return;
    state.switching = true;
    const shrinking = VIEW_RANK[next] < VIEW_RANK[state.view];
    try {
      // Shrink the content before the window, grow the window before the content: nothing gets clipped mid-way.
      if (shrinking) applyView(next);
      if (host) placeCorners(await host.layout(next, state.scale));
      if (!shrinking) applyView(next);
      saveUi();
      if (next === "mini") $("puck").focus({ preventScroll: true });
      else if (document.activeElement === $("puck") || document.activeElement === document.body) $("stage").focus({ preventScroll: true });
    } catch (error) {
      console.warn("view change failed:", error);
    } finally { state.switching = false; }
  }

  function setOpacity(percent) {
    state.opacity = Math.min(100, Math.max(30, Math.round(Number(percent) || 80)));
    document.documentElement.style.setProperty("--op", String(state.opacity / 100));
    $("opacity").value = String(state.opacity);
  }

  // The widget scales as a whole (CSS zoom); the native window is resized to fit it.
  let scaleTimer;
  function setScale(value, apply = true) {
    state.scale = Math.min(1.6, Math.max(0.7, Math.round((Number(value) || 1) * 10) / 10));
    document.documentElement.style.setProperty("--scale", String(state.scale));
    $("scale").value = String(Math.round(state.scale * 100));
    if (!apply) return;
    saveUi();
    if (!host) return;
    clearTimeout(scaleTimer);
    scaleTimer = setTimeout(async () => {
      if (state.switching) return;
      state.switching = true;
      try { placeCorners(await host.layout(state.view, state.scale)); }
      catch (error) { console.warn("resize failed:", error); }
      finally { state.switching = false; }
    }, 40);
  }

  // ---------- input ----------
  function onWheel(event, forcePrompt) {
    if (!state.categories.length || !event.deltaY) return;
    event.preventDefault();
    const now = Date.now();
    if (event.ctrlKey) {
      if (now - state.genreAt >= 60) { state.genreAt = now; setScale(state.scale - Math.sign(event.deltaY) * 0.1); }
      return;
    }
    if (forcePrompt || event.target.closest(".core")) {
      state.promptAccum += event.deltaY; state.genreAccum = 0;
      if (Math.abs(state.promptAccum) >= 50 && now - state.promptAt >= 130) {
        const direction = Math.sign(state.promptAccum);
        state.promptAccum = 0; state.promptAt = now;
        spinPrompt(direction);
      }
    } else {
      state.genreAccum += event.deltaY; state.promptAccum = 0;
      if (Math.abs(state.genreAccum) >= 60 && now - state.genreAt >= 150) {
        const direction = Math.sign(state.genreAccum);
        state.genreAccum = 0; state.genreAt = now;
        spinGenre(direction);
      }
    }
  }

  function bind() {
    $("core").addEventListener("click", copyCurrent);
    $("puck").addEventListener("click", () => setView("ring"));
    $("btnMini").addEventListener("click", () => setView("mini"));
    $("btnList").addEventListener("click", () => setView(state.view === "drawer" ? "ring" : "drawer"));
    $("btnFav").addEventListener("click", () => toggleFavorite(currentPrompt()));
    $("stage").addEventListener("wheel", (event) => onWheel(event, false), { passive: false });
    $("puck").addEventListener("wheel", (event) => onWheel(event, true), { passive: false });

    if (host) {
      const pin = $("btnPin");
      pin.hidden = false;
      const showPin = (pinned) => {
        pin.setAttribute("aria-pressed", String(pinned));
        pin.title = pinned ? "最前面固定を解除" : "常に最前面に固定";
      };
      showPin(host.pinned);
      pin.addEventListener("click", async () => showPin(await host.togglePin()));
    }

    $("search").addEventListener("input", (event) => { state.search = event.target.value; renderList(); });
    $("favOnly").addEventListener("click", () => {
      state.favoritesOnly = !state.favoritesOnly;
      $("favOnly").setAttribute("aria-pressed", String(state.favoritesOnly));
      renderList();
    });
    $("opacity").addEventListener("input", (event) => { setOpacity(event.target.value); saveUi(); });
    $("scale").addEventListener("change", (event) => setScale(Number(event.target.value) / 100));
    $("addPrompt").addEventListener("click", () => { state.editing = { id: null }; renderList(); });

    // Resting the pointer on the centre brings the prompt text up; leaving puts it away.
    let hoverTimer;
    $("core").addEventListener("mouseenter", () => { hoverTimer = setTimeout(() => peek(0), 320); });
    $("core").addEventListener("mouseleave", () => { clearTimeout(hoverTimer); hideBubble(); });

    document.addEventListener("keydown", async (event) => {
      const typing = event.target instanceof Element && event.target.matches("input, textarea, select");
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        await setView("drawer");
        $("search").focus();
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        if (state.editing) { state.editing = null; renderList(); return; }
        if (typing && $("search").value) { $("search").value = ""; state.search = ""; renderList(); return; }
        void setView(state.view === "drawer" ? "ring" : "mini");
        return;
      }
      if (typing || event.ctrlKey || event.metaKey || event.altKey || state.view === "mini") return;
      const onButton = event.target instanceof HTMLButtonElement && event.target.id !== "core";
      switch (event.key) {
        case "ArrowRight": event.preventDefault(); spinGenre(1); break;
        case "ArrowLeft": event.preventDefault(); spinGenre(-1); break;
        case "ArrowDown": event.preventDefault(); spinPrompt(1); break;
        case "ArrowUp": event.preventDefault(); spinPrompt(-1); break;
        case "Enter": case " ":
          if (!onButton && event.target.id !== "core") { event.preventDefault(); void copyCurrent(); }
          break;
        case "f": case "F": toggleFavorite(currentPrompt()); break;
        default: break;
      }
    });
  }

  async function start() {
    const savedFavorites = readJson(FAVORITES_KEY, []);
    if (Array.isArray(savedFavorites)) state.favorites = new Set(savedFavorites.filter((x) => typeof x === "string"));
    const ui = readJson(UI_KEY, {}) || {};
    const initialView = ui.view in VIEW_RANK ? ui.view : "ring";
    bind();
    setOpacity(ui.opacity ?? 80);
    setScale(ui.scale ?? 1, false);
    loadCustom();
    placeCorners(null);
    try {
      const response = await fetch("./data/prompts.json", { cache: "no-store" });
      if (!response.ok) throw new Error("HTTP " + response.status);
      const catalog = await response.json();
      if (!Array.isArray(catalog.categories) || !Array.isArray(catalog.prompts)) throw new Error("データ形式が不正");
      state.categories = catalog.categories;
      state.base = catalog.prompts;
      rebuildPrompts();
      buildRing();
      render();
    } catch (error) {
      $("coreGenre").textContent = "読み込み失敗";
      $("coreTitle").textContent = "HTTPサーバー経由で開いてください";
      $("core").title = String(error.message || error);
    }
    // Restore the last view; the native window is revealed only after it has its final size and place.
    applyView(initialView);
    if (host) {
      try { placeCorners(await host.ready(initialView, state.scale)); }
      catch (error) { console.warn("native restore failed:", error); }
    }
  }
  start();
})();
