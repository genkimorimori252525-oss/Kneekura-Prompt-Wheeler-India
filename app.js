"use strict";
(() => {
  const FAVORITES_KEY = "kneekura.prompt-wheel.favorites.v1";
  const state = { categories: [], prompts: [], turn: 0, search: "", favoritesOnly: false, favorites: new Set(), wheelAccum: 0, wheelAt: 0, promptIndices: Object.create(null), promptAccum: 0, promptAt: 0 };
  const $ = (id) => document.getElementById(id);
  const modulo = (n, m) => ((n % m) + m) % m;
  const make = (tag, className, value) => {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (value !== undefined) node.textContent = value;
    return node;
  };
  const sourceBadge = (kind) => kind === "user-supplied" ? "提供画像" : kind === "adapted" ? "Reddit参考・再構成" : "オリジナル";
  const currentIndex = () => modulo(state.turn, state.categories.length || 1);
  const currentCategory = () => state.categories[currentIndex()];

  function readFavorites() {
    try {
      const value = JSON.parse(localStorage.getItem(FAVORITES_KEY) || "[]");
      if (Array.isArray(value)) return new Set(value.filter((x) => typeof x === "string"));
    } catch (_) { /* Storage may be disabled. */ }
    return new Set();
  }

  function saveFavorites() {
    try { localStorage.setItem(FAVORITES_KEY, JSON.stringify([...state.favorites])); }
    catch (_) { /* Private browsing may prevent persistence. */ }
  }

  let toastTimer;
  function toast(message) {
    const target = $("toast");
    target.textContent = message;
    target.classList.add("show");
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => target.classList.remove("show"), 2400);
  }

  async function copyText(value) {
    try {
      if (navigator.clipboard && window.isSecureContext) {
        await navigator.clipboard.writeText(value);
        return true;
      }
    } catch (_) { /* Try the standard fallback below. */ }
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

  function quickPrompt() {
    const category = currentCategory();
    if (!category) return null;
    const choices = state.prompts.filter((p) => p.category === category.id);
    if (!choices.length) return null;
    const index = modulo(state.promptIndices[category.id] || 0, choices.length);
    return choices[index];
  }

  function updateQuickPrompt() {
    const picked = quickPrompt();
    $("quickPromptTitle").textContent = picked ? picked.title : "プロンプトなし";
    $("quickCopy").disabled = !picked;
    $("quickCopy").setAttribute("aria-label", picked ? picked.title + "をコピー" : "コピー不可");
  }

  function spinPrompt(direction) {
    const category = currentCategory();
    if (!category) return;
    const options = state.prompts.filter((p) => p.category === category.id);
    if (!options.length) return;
    state.promptIndices[category.id] = modulo((state.promptIndices[category.id] || 0) + direction, options.length);
    updateQuickPrompt();
    renderCards();
  }

  function showRing() {
    const categories = state.categories;
    if (!categories.length) return;
    const n = categories.length, step = 360 / n;
    const clockwise = [categories[0], ...categories.slice(1).reverse()];
    const parts = clockwise.map((cat, i) => {
      const start = i * step, end = (i + 1) * step;
      return cat.color + " " + start + "deg " + (end - 1.5) + "deg, #11192b " + (end - 1.5) + "deg " + end + "deg";
    });
    $("wheelRotor").style.background = "conic-gradient(from -" + step / 2 + "deg, " + parts.join(", ") + ")";
    $("wheelRotor").style.transform = "rotate(" + state.turn * step + "deg)";

    const center = currentCategory();
    $("currentIcon").textContent = center.icon;
    $("currentIcon").style.color = center.color;
    $("currentLabel").textContent = center.short;
    $("currentCount").textContent = state.prompts.filter((p) => p.category === center.id).length + " PROMPTS";
    updateQuickPrompt();
    $("wheelStage").setAttribute("aria-label", "ジャンルホイール。現在は" + center.label + "。ホイール、左右キー、各ジャンルを選択できます");

    for (const node of $("orbit").children) {
      const i = Number(node.dataset.index);
      const angle = (-90 - i * step + state.turn * step) * Math.PI / 180;
      node.style.left = (50 + Math.cos(angle) * 34) + "%";
      node.style.top = (50 + Math.sin(angle) * 34) + "%";
      node.setAttribute("aria-pressed", String(i === currentIndex()));
    }
  }

  function spin(direction) {
    state.turn += direction;
    showRing();
    renderCards();
  }

  function moveTo(index) {
    const n = state.categories.length;
    let delta = modulo(index - currentIndex(), n);
    if (delta > n / 2) delta -= n;
    state.turn += delta;
    showRing();
    renderCards();
  }

  function createOrbit() {
    const orbit = $("orbit");
    orbit.replaceChildren();
    state.categories.forEach((cat, i) => {
      const button = make("button", "orbit-button");
      button.type = "button";
      button.dataset.index = String(i);
      button.style.setProperty("--cat-color", cat.color);
      button.setAttribute("aria-label", cat.label + "を選ぶ");
      button.append(make("span", "orbit-icon", cat.icon), make("span", "orbit-name", cat.short));
      button.addEventListener("click", () => moveTo(i));
      orbit.append(button);
    });
    showRing();
  }

  function cardFor(prompt) {
    const card = make("article", "prompt-card");
    if (prompt.id === quickPrompt()?.id) card.classList.add("quick-selected");
    const top = make("div", "card-top");
    const info = make("div", "card-info");
    info.append(make("h4", "card-title", prompt.title), make("p", "card-summary", prompt.summary));
    const fav = make("button", "star-button", state.favorites.has(prompt.id) ? "★" : "☆");
    fav.type = "button";
    fav.title = "お気に入りを切り替え";
    fav.setAttribute("aria-label", prompt.title + "をお気に入りに追加または削除");
    fav.setAttribute("aria-pressed", String(state.favorites.has(prompt.id)));
    fav.addEventListener("click", () => {
      if (state.favorites.has(prompt.id)) state.favorites.delete(prompt.id);
      else state.favorites.add(prompt.id);
      saveFavorites();
      renderCards();
    });
    top.append(info, fav);
    card.append(top);

    const chips = make("div", "chips");
    const cat = state.categories.find((c) => c.id === prompt.category);
    chips.append(make("span", "chip", cat?.short || prompt.category), make("span", "chip source", sourceBadge(prompt.source.type)));
    card.append(chips);

    const actions = make("div", "card-actions");
    const copyJa = make("button", "copy-button", "コピーする ↗");
    copyJa.type = "button";
    copyJa.setAttribute("aria-label", prompt.title + "の日本語版をコピー");
    copyJa.addEventListener("click", async () => toast(await copyText(prompt.prompt) ? "日本語プロンプトをコピーしたで" : "コピーできませんでした。本文を開いて手動でコピーしてください"));
    actions.append(copyJa);
    if (prompt.promptEn) {
      const copyEn = make("button", "copy-en", "EN 原文コピー");
      copyEn.type = "button";
      copyEn.addEventListener("click", async () => toast(await copyText(prompt.promptEn) ? "英語原文をコピーしました" : "コピーに失敗しました"));
      actions.append(copyEn);
    }
    card.append(actions);

    const details = make("details", "prompt-details");
    const summary = make("summary", "", "プロンプト本文を見る");
    details.append(summary, make("p", "", prompt.prompt));
    if (prompt.promptEn) {
      const en = make("p", "", prompt.promptEn);
      en.setAttribute("lang", "en");
      details.append(en);
    }
    if (prompt.note) details.append(make("p", "prompt-note", "注意: " + prompt.note));
    card.append(details);

    if (prompt.source?.url?.startsWith("https://")) {
      const link = make("a", "source-link", prompt.source.type === "user-supplied" ? "作者プロフィール（個別投稿URL未確認）" : "参考元の投稿を見る");
      link.href = prompt.source.url;
      link.target = "_blank";
      link.rel = "noopener noreferrer";
      card.append(link);
    }
    return card;
  }

  function renderCards() {
    if (!state.categories.length) return;
    const selected = currentCategory();
    const searched = state.search.trim().toLocaleLowerCase("ja");
    const showAll = searched.length > 0 || state.favoritesOnly;
    const visible = state.prompts.filter((p) => {
      if (!showAll && p.category !== selected.id) return false;
      if (state.favoritesOnly && !state.favorites.has(p.id)) return false;
      if (!searched) return true;
      const category = state.categories.find((c) => c.id === p.category);
      const text = [p.title, p.summary, p.prompt, p.promptEn || "", category?.label || "", p.source.label].join(" ").toLocaleLowerCase("ja");
      return text.includes(searched);
    });

    $("selectionTitle").textContent = searched ? "検索結果" : state.favoritesOnly ? "お気に入り" : selected.label;
    $("selectionDesc").textContent = searched ? "すべてのジャンルから探しています" : state.favoritesOnly ? "保存したプロンプトを一覧表示" : selected.desc;
    $("selectionMark").style.background = showAll ? "#9eacf3" : selected.color;
    $("resultsCount").textContent = visible.length + " 件";
    const cards = $("cards");
    cards.replaceChildren();
    if (!visible.length) {
      const message = searched ? "一致するプロンプトがありません。検索語を変えてみてください。" : state.favoritesOnly ? "お気に入りはまだありません。☆ を押して追加できます。" : "このジャンルのプロンプトは未登録です。";
      cards.append(make("p", "empty", message));
    } else {
      for (const p of visible) cards.append(cardFor(p));
    }
  }

  function bind() {
    $("quickCopy").addEventListener("click", async () => {
      const picked = quickPrompt();
      if (picked) toast(await copyText(picked.prompt) ? "プロンプトをコピーしました" : "コピーに失敗しました");
    });
    $("prevCategory").addEventListener("click", () => spin(-1));
    $("nextCategory").addEventListener("click", () => spin(1));
    $("wheelStage").addEventListener("wheel", (event) => {
      if (!state.categories.length || !event.deltaY) return;
      event.preventDefault();
      state.wheelAccum += event.deltaY;
      const now = Date.now();
      const inCenter = document.body.classList.contains("desktop-mode") && !!event.target.closest(".wheel-center");
      if (inCenter) {
        state.promptAccum += event.deltaY;
        state.wheelAccum = 0;
        if (Math.abs(state.promptAccum) >= 55 && now - state.promptAt >= 155) {
          const direction = Math.sign(state.promptAccum);
          state.promptAccum = 0;
          state.promptAt = now;
          spinPrompt(direction);
        }
      } else if (Math.abs(state.wheelAccum) >= 65 && now - state.wheelAt >= 160) {
        const direction = Math.sign(state.wheelAccum);
        state.wheelAccum = 0;
        state.promptAccum = 0;
        state.wheelAt = now;
        spin(direction);
      }
    }, { passive: false });
    $("wheelStage").addEventListener("keydown", (event) => {
      if (event.key === "ArrowRight" || event.key === "ArrowDown") {
        event.preventDefault(); if (event.altKey) spinPrompt(1); else spin(1);
      }
      if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
        event.preventDefault(); if (event.altKey) spinPrompt(-1); else spin(-1);
      }
    });
    $("searchInput").addEventListener("input", (event) => {
      state.search = event.target.value;
      renderCards();
    });
    $("favoriteFilter").addEventListener("click", () => {
      state.favoritesOnly = !state.favoritesOnly;
      $("favoriteFilter").setAttribute("aria-pressed", String(state.favoritesOnly));
      $("favoriteFilter").textContent = state.favoritesOnly ? "★ お気に入り中" : "☆ お気に入り";
      renderCards();
    });
    document.addEventListener("keydown", (event) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault(); $("searchInput").focus();
      }
    });
  }

  async function start() {
    bind();
    state.favorites = readFavorites();
    try {
      const response = await fetch("./data/prompts.json", { cache: "no-store" });
      if (!response.ok) throw new Error("HTTP " + response.status);
      const catalog = await response.json();
      if (!Array.isArray(catalog.categories) || !Array.isArray(catalog.prompts)) throw new Error("データ形式が不正");
      state.categories = catalog.categories;
      state.prompts = catalog.prompts;
      $("totalCount").textContent = state.prompts.length + " PROMPTS";
      createOrbit();
      renderCards();
    } catch (error) {
      $("cards").replaceChildren(make("p", "empty", "読み込みに失敗しました。GitHub PagesまたはローカルHTTPサーバーから開いてください。原因: " + error.message));
      $("totalCount").textContent = "読み込み失敗";
    }
  }
  start();
})();
