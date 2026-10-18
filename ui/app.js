"use strict";
const { invoke } = window.__TAURI__.core;
const $ = (s) => document.querySelector(s);
const el = (tag, props = {}, ...kids) => {
  const n = Object.assign(document.createElement(tag), props);
  n.append(...kids);
  return n;
};
const store = {
  get(k, d) { try { return JSON.parse(localStorage.getItem("bs." + k)) ?? d; } catch { return d; } },
  set(k, v) { try { localStorage.setItem("bs." + k, JSON.stringify(v)); } catch { /* storage unavailable */ } },
};

const OT_CODES = new Set("Gen Exo Lev Num Deu Jos Jdg Rut 1Sa 2Sa 1Ki 2Ki 1Ch 2Ch Ezr Neh Est Job Psa Pro Ecc Sol Isa Jer Lam Eze Dan Hos Joe Amo Oba Jon Mic Nah Hab Zep Hag Zec Mal".split(" "));
const NT_CODES = new Set("Mat Mar Luk Joh Act Rom 1Co 2Co Gal Eph Phi Col 1Th 2Th 1Ti 2Ti Tit Phm Heb Jam 1Pe 2Pe 1Jo 2Jo 3Jo Jud Rev".split(" "));
const LABELS = {
  byz: "Byzantine NT", bgt: "LXX + NT", bnt: "NA27 NT", gnt: "UBS NT", scr: "Scrivener NT", tis: "Tischendorf NT",
  ste: "Stephanus NT", who: "Westcott-Hort NT", goc: "Greek Orthodox NT", lxt: "Rahlfs LXX", wtt: "Hebrew OT",
};
const GROUPS = { latin: "English & other", greek: "Greek", hebrew: "Hebrew" };
const LANG = { latin: "en", greek: "grc", hebrew: "he" };

const S = {
  versions: [], // [{id, script}]
  books: [], // books of the primary version, in its own structure
  v1: "kjv", v2: "", book: 0, chapter: 1, cant: false,
  data: null, sel: null, navBook: 0, req: 0, searchVersion: "kjv",
};
const scriptOf = (id) => S.versions.find((v) => v.id === id)?.script ?? "latin";
const reader = $("#reader"), main = $("#main"), panel = $("#panel");
const bookName = (i) => S.books[i]?.name ?? "";
const label = (b, c, v) => `${bookName(b)} ${c}${v ? ":" + v : ""}`;
const strongsLabel = (s, code) => (OT_CODES.has(code) ? "H" : "G") + s.replace(/^0+/, "");
const save = () => store.set("state", { v1: S.v1, v2: S.v2, code: S.books[S.book]?.code, chapter: S.chapter, cant: S.cant });
const cols = () => (S.v2 ? [S.v1, S.v2] : [S.v1]);

/* ── tokens → DOM (never innerHTML: module text is data) ── */
function renderTokens(tokens, { words = false, col = 0, vi = 0 } = {}) {
  const frag = document.createDocumentFragment();
  tokens.forEach((t, i) => {
    if (words && (t.s || t.w != null)) {
      const w = el("span", { className: "w", textContent: t.t });
      Object.assign(w.dataset, { i, col, vi });
      if (t.s) w.dataset.s = t.s;
      frag.append(w);
    } else if (t.h) frag.append(el("mark", { textContent: t.t }));
    else frag.append(t.t);
  });
  return frag;
}

/* ── fonts that ship with BibleWorks, for pointed Hebrew and polytonic Greek ── */
async function loadFont(name, family) {
  try {
    const face = new FontFace(family, await invoke("font", { name }));
    document.fonts.add(await face.load());
  } catch { /* fall back to system fonts */ }
}

/* ── chapter ── */
async function loadChapter(scrollVerse) {
  const req = ++S.req;
  const slow = setTimeout(() => {
    if (req === S.req) reader.replaceChildren(el("div", { className: "skeleton", ariaHidden: "true" }, ...Array.from({ length: 9 }, () => el("i"))));
  }, 150);
  let data;
  try {
    data = await invoke("chapter", { version: S.v1, book: S.book, chapter: S.chapter, compare: S.v2 ? [S.v2] : [], cantillation: S.cant });
  } catch (e) {
    clearTimeout(slow);
    if (req === S.req) reader.replaceChildren(el("div", { className: "empty" }, el("h2", { textContent: "Couldn't load this chapter" }), el("p", { textContent: String(e) })));
    return;
  }
  clearTimeout(slow);
  if (req !== S.req) return;
  S.data = data;
  S.sel = scrollVerse ? { verse: scrollVerse, col: 0, vi: 0, idx: null } : null;
  renderChapter();
  $("#prev").disabled = !data.prev;
  $("#next").disabled = !data.next;
  $("#title").textContent = label(S.book, S.chapter);
  document.title = `${label(S.book, S.chapter)} · Bible Study`;
  $("#cant-btn").hidden = !cols().some((v) => scriptOf(v) === "hebrew");
  $("#cant-btn").setAttribute("aria-pressed", String(S.cant));
  save();
  renderWord();
  if (typeof markNotes === "function") { markNotes(); notesChanged(); }
  if (scrollVerse) reader.querySelector(`.verse[data-v="${scrollVerse}"]`)?.scrollIntoView({ block: "center" });
  else reader.scrollTop = 0;
  if (!document.activeElement || document.activeElement === document.body) reader.focus({ preventScroll: true });
}

function renderChapter() {
  const c = cols();
  const wrap = el("div", { className: "chapter" + (c.length > 1 ? " two" : "") });
  wrap.style.setProperty("--cols", c.length);
  if (scriptOf(c[0]) === "hebrew") wrap.dataset.rtl = "";
  if (c.length > 1) wrap.append(el("div", { className: "cols-head" }, ...c.map((v) => el("span", { textContent: v.toUpperCase() }))));
  for (const r of S.data.rows) {
    const row = el("div", { className: "verse" });
    row.dataset.v = r.n;
    row.append(el("button", { className: "vnum", textContent: r.n, ariaLabel: `Verse ${r.n}` }));
    r.cells.forEach((cell, col) => {
      const sc = scriptOf(c[col]);
      const td = el("div", { className: `vtext ${sc}`, lang: LANG[sc], dir: sc === "hebrew" ? "rtl" : "ltr" });
      if (!cell.length) td.append(el("span", { className: "none", textContent: "—", title: "No matching verse in this version" }));
      cell.forEach((v, vi) => {
        if (vi > 0) td.append(" ");
        if (col > 0 && (v.n !== r.n || cell.length > 1)) td.append(el("sup", { className: "vref", textContent: v.n }), " ");
        td.append(renderTokens(v.tokens, { words: true, col, vi }));
      });
      row.append(td);
    });
    wrap.append(row);
  }
  reader.replaceChildren(wrap);
  markSelection();
}

function markSelection() {
  reader.querySelectorAll(".sel, .on").forEach((n) => n.classList.remove("sel", "on"));
  if (!S.sel) return;
  const row = reader.querySelector(`.verse[data-v="${S.sel.verse}"]`);
  row?.classList.add("sel");
  if (S.sel.idx != null) row?.querySelectorAll(`.w[data-col="${S.sel.col}"][data-vi="${S.sel.vi}"][data-i="${S.sel.idx}"]`).forEach((w) => w.classList.add("on"));
}

reader.addEventListener("click", (e) => {
  const row = e.target.closest(".verse");
  if (!row) return;
  const w = e.target.closest(".w");
  S.sel = { verse: +row.dataset.v, col: w ? +w.dataset.col : 0, vi: w ? +w.dataset.vi : 0, idx: w ? +w.dataset.i : null };
  markSelection();
  renderWord();
  setPanel(true);
  if ($("#tab-notes").getAttribute("aria-selected") === "true" || $("#tab-refs").getAttribute("aria-selected") === "true") notesChanged();
  else selectTab("word");
});

/* ── analysis pane ── */
function selectedCell() {
  const { verse, col, vi } = S.sel;
  return S.data?.rows[verse - 1]?.cells[col]?.[vi];
}
const selectedTokens = () => selectedCell()?.tokens ?? [];

/** Styled lexicon text: Greek/Hebrew runs get their own fonts, references become links. */
function renderSpans(spans) {
  const box = el("div", { className: "lexentry" });
  for (const s of spans) {
    if (s.link) {
      const b = el("button", { className: "reflink", type: "button", textContent: s.t });
      const [code, ch, v] = s.link;
      b.addEventListener("click", () => goToCode(code, ch, v, OT_CODES.has(code) && S.versions.some((x) => x.id === "wtt") ? "wtt" : "kjv"));
      box.append(b);
      continue;
    }
    const n = el("span", { textContent: s.t });
    if (s.b) n.classList.add("b");
    if (s.i) n.classList.add("i");
    if (s.g) { n.classList.add("greek"); n.lang = "grc"; }
    if (s.h) { n.classList.add("hebrew"); n.lang = "he"; n.dir = "rtl"; }
    box.append(n);
  }
  return box;
}

/** Strong's number, pronunciation and definition. */
function strongsCard(e) {
  const defn = el("div", { className: "def", textContent: e.body });
  const head = el("div", { className: "sn-head" }, el("b", { textContent: e.label }), ` ${e.translit}`, e.pronunciation ? el("span", { className: "pron", textContent: ` ${e.pronunciation}` }) : "");
  return el("div", { className: "sn" }, head, defn);
}

/** Tabs for a set of lexicons; the first is looked up immediately, the rest on demand. */
function lexSection(lexicons, args) {
  const out = el("div", { className: "lexsec" });
  const tabs = el("div", { className: "lextabs", role: "tablist", ariaLabel: "Lexicons" });
  const body = el("div", { className: "lexbody", role: "tabpanel" });
  const cache = new Map();
  let current = 0;
  const show = async (i) => {
    current = i;
    [...tabs.children].forEach((t, k) => t.setAttribute("aria-selected", String(k === i)));
    body.replaceChildren(el("p", { className: "muted", textContent: "Looking up…" }));
    const lx = lexicons[i];
    try {
      if (!cache.has(lx.id)) cache.set(lx.id, await invoke("lookup", { lexicon: lx.id, ...args }));
      if (current !== i) return;
      const entries = cache.get(lx.id);
      if (!entries.length) { body.replaceChildren(el("p", { className: "muted", textContent: `No ${lx.name} entry for this word.` })); return; }
      body.replaceChildren(...entries.map((spans, k) => el("div", { className: "lexitem" }, entries.length > 1 ? el("small", { className: "muted", textContent: `Entry ${k + 1} of ${entries.length}` }) : "", renderSpans(spans))));
    } catch (e) {
      if (current === i) body.replaceChildren(el("p", { className: "muted", textContent: String(e) }));
    }
  };
  lexicons.forEach((lx, i) => {
    const b = el("button", { type: "button", role: "tab", textContent: lx.name, ariaSelected: "false" });
    b.addEventListener("click", () => show(i));
    tabs.append(b);
  });
  out.append(tabs, body);
  show(0);
  return out;
}

let wreq = 0;

async function renderWord() {
  const pane = $("#pane-word");
  const mine = ++wreq;
  if (!S.sel) {
    pane.replaceChildren(el("div", { className: "panel-empty" }, el("strong", { textContent: "Nothing selected" }), "Click a word or a verse number to analyze it."));
    return;
  }
  const { verse, col, idx } = S.sel;
  const version = cols()[col] ?? S.v1;
  const sc = scriptOf(version);
  const cell = selectedCell();
  const toks = cell?.tokens ?? [];
  const where = label(S.book, S.chapter, verse); // the primary version's reference
  const head = el("div", { className: "word-head" }, el("h2", { textContent: `${where} · ${version.toUpperCase()}` }));
  const select = (i) => () => { S.sel = { ...S.sel, idx: i }; markSelection(); renderWord(); };

  if (sc !== "latin") {
    const words = toks.map((t, i) => ({ t, i })).filter((x) => x.t.w != null);
    const chips = el("div", { className: "chips", role: "group", ariaLabel: "Words in this verse" }, ...words.map(({ t, i }) => {
      const b = el("button", { className: `chip ${sc}`, type: "button", ariaPressed: String(i === idx), lang: LANG[sc], dir: sc === "hebrew" ? "rtl" : "ltr" }, t.t);
      b.addEventListener("click", select(i));
      return b;
    }));
    pane.replaceChildren(head, chips);
    if (idx == null || toks[idx]?.w == null) {
      pane.append(el("div", { className: "panel-empty" }, el("strong", { textContent: "Pick a word" }), "Choose a word above, or click one in the text, for its parsing and dictionary entries."));
      return;
    }
    pane.append(el("p", { className: "muted pad", textContent: "Looking up…" }));
    let info;
    try { info = await invoke("word_info", { version, book: cell.b, chapter: cell.c, verse: cell.n, word: toks[idx].w }); }
    catch (e) { if (mine === wreq) pane.lastChild.textContent = `No parsing for this version (${e}).`; return; }
    if (mine !== wreq) return;
    pane.lastChild.remove();
    pane.append(originalCard(info, sc, toks[idx].t));
    return;
  }

  const code = S.books[S.book]?.code;
  const tagged = toks.map((t, i) => ({ t, i })).filter((x) => x.t.s);
  if (!tagged.length) {
    pane.replaceChildren(head, el("div", { className: "panel-empty" }, el("strong", { textContent: "No word tags in this version" }), "Switch to a version with Strong’s numbers, such as KJV or NKJ."));
    return;
  }
  const chips = el("div", { className: "chips", role: "group", ariaLabel: "Words in this verse" }, ...tagged.map(({ t, i }) => {
    const b = el("button", { className: "chip", type: "button", ariaPressed: String(i === idx) }, t.t.trim(), el("small", { textContent: strongsLabel(t.s, code) }));
    b.addEventListener("click", select(i));
    return b;
  }));
  pane.replaceChildren(head, chips);
  if (idx == null || !toks[idx]?.s) return;
  const t = toks[idx], hebrew = OT_CODES.has(code), number = parseInt(t.s, 10);
  const card = el("div", { className: "detail" }, el("p", { className: "muted", textContent: "Looking up…" }));
  pane.append(card);
  let info;
  try { info = await invoke("strongs_info", { number, hebrew }); } catch { info = null; }
  if (mine !== wreq) return;
  if (!info) { card.replaceChildren(el("p", { className: "muted", textContent: `No Strong’s entry for ${strongsLabel(t.s, code)}.` })); return; }
  const count = el("span", { className: "muted", textContent: "" });
  const find = el("button", { className: "btn", type: "button", textContent: `Find all ${info.strongs.label}` });
  find.addEventListener("click", () => runSearch(info.strongs.label));
  card.replaceChildren(
    el("div", { className: "big-word" }, el("span", { className: "greek", lang: "grc", textContent: info.lemma }), el("span", { className: "muted", textContent: info.lemma ? "" : t.t.trim() })),
    strongsCard(info.strongs),
    el("div", { className: "actions" }, find, count),
    lexSection(info.lexicons, { lemma: info.lemma, strongs: [number] }),
  );
  invoke("find", { version, query: info.strongs.label }).then((r) => { if (mine === wreq) count.textContent = `${r.hits.length}${r.capped ? "+" : ""} verses in ${version.toUpperCase()}`; }).catch(() => {});
}

/** The analysis card for a Greek or Hebrew word: parsing, lemma, Strong's, lexicons. */
function originalCard(info, sc, text) {
  const card = el("div", { className: "detail" });
  const cls = sc === "hebrew" ? "hebrew" : "greek";
  card.append(el("div", { className: "big-word" }, el("span", { className: cls, lang: LANG[sc], dir: sc === "hebrew" ? "rtl" : "ltr", textContent: text })));
  let lexArgs = null;
  info.parts.forEach((p) => {
    const part = el("div", { className: "part" },
      el("div", { className: "part-top" }, el("span", { className: `lemma ${cls}`, lang: LANG[sc], dir: sc === "hebrew" ? "rtl" : "ltr", textContent: p.lemma }), el("span", { className: "parsing", textContent: p.parsing })));
    p.strongs.forEach((e) => part.append(strongsCard(e)));
    card.append(part);
    // Dictionary lookups follow the word's main morpheme (the last one that isn't a particle).
    if (!p.particle) lexArgs = { lemma: sc === "hebrew" ? p.lemma_raw : p.lemma, strongs: p.strongs.map((e) => e.number) };
  });
  const mainPart = [...info.parts].reverse().find((p) => !p.particle) ?? info.parts[0];
  const letters = (s) => s.replace(/\u05BE/g, " ").normalize("NFD").replace(/[\u0300-\u036F\u0591-\u05C7]/g, "").replace(/[^\p{L} ]/gu, "").trim();
  const form = letters(text);
  const actions = el("div", { className: "actions" });
  if (mainPart?.lemma) {
    const all = el("button", { className: "btn small", type: "button", textContent: "Find all forms" });
    all.addEventListener("click", () => runSearch(`${mainPart.lemma}@*`));
    actions.append(all);
  }
  if (form) {
    const one = el("button", { className: "btn small ghost", type: "button", textContent: "Find this form" });
    one.addEventListener("click", () => runSearch(form.includes(" ") ? `"${form}"` : form));
    actions.append(one);
  }
  card.append(actions);
  if (lexArgs) card.append(lexSection(info.lexicons, lexArgs));
  return card;
}

/* ── search ── */
async function runSearch(query) {
  const pane = $("#pane-search");
  setPanel(true);
  selectTab("search");
  const sc = scriptOf(S.v1);
  S.searchVersion = S.v1;
  pane.replaceChildren(el("p", { className: "sum", textContent: sc === "latin" || S.indexed?.has(S.v1) ? "Searching…" : "Searching… (the first Greek or Hebrew search of a version prepares an index)" }));
  store.set("history", [query, ...store.get("history", []).filter((h) => h !== query)].slice(0, 8));
  let r;
  try { r = await invoke("find", { version: S.v1, query }); }
  catch (e) { pane.replaceChildren(el("div", { className: "panel-empty" }, el("strong", { textContent: "Couldn’t run that search" }), String(e))); return; }
  (S.indexed ??= new Set()).add(S.v1);
  if (!r.hits.length) {
    const tip = sc === "latin" ? "Try fewer words, or a wildcard like love*." : "Accents and vowel points are ignored unless you type them. Use lemma@* (for example ἀγαπάω@*) to find every form of a word.";
    pane.replaceChildren(el("div", { className: "panel-empty" }, el("strong", { textContent: `No verses match “${query}”` }), `Searched ${S.v1.toUpperCase()}. ${tip}`));
    return;
  }
  const sum = el("p", { className: "sum", textContent: `${r.hits.length}${r.capped ? "+" : ""} verses · “${query}” · ${S.v1.toUpperCase()}${r.capped ? " (showing first 500)" : ""}` });
  const list = r.hits.map((h) => {
    const b = el("button", { className: "hit", type: "button" }, el("b", { textContent: `${h.book} ${h.chapter}:${h.verse}` }), el("span", { className: sc === "latin" ? "" : `hit-text ${sc}`, lang: LANG[sc], dir: sc === "hebrew" ? "rtl" : "ltr" }, renderTokens(h.tokens)));
    b.addEventListener("click", () => {
      list.forEach((x) => x.classList.remove("sel"));
      b.classList.add("sel");
      goToCode(h.code, h.chapter, h.verse, S.searchVersion);
    });
    return b;
  });
  const slot = el("div");
  const saveBtn = el("button", { className: "btn small ghost", type: "button", textContent: "Save as verse list" });
  saveBtn.addEventListener("click", () => saveHitsAsList(S.searchVersion, r.hits, query, slot));
  pane.replaceChildren(sum, el("div", { className: "toolbar" }, saveBtn), slot, ...list);
  pane.scrollTop = 0;
}

/* ── panel + tabs ── */
function setPanel(open) {
  panel.hidden = !open;
  main.classList.toggle("with-panel", open);
  $("#panel-btn").setAttribute("aria-pressed", String(open));
}
const TABS = ["search", "word", "refs", "notes", "lists"];
function selectTab(name) {
  if (name !== "notes" && typeof flushNote === "function") flushNote().catch(() => {});
  for (const t of TABS) {
    const on = t === name;
    $(`#tab-${t}`).setAttribute("aria-selected", String(on));
    $(`#tab-${t}`).tabIndex = on ? 0 : -1;
    $(`#pane-${t}`).hidden = !on;
  }
  if (name === "refs" && typeof renderRefs === "function") renderRefs();
  if (name === "notes" && typeof renderNotes === "function") renderNotes();
  if (name === "lists" && typeof renderLists === "function") renderLists();
}
$("#panel-btn").addEventListener("click", () => setPanel(panel.hidden));
$("#panel-close").addEventListener("click", () => setPanel(false));
$("#tab-search").addEventListener("click", () => selectTab("search"));
$("#tab-word").addEventListener("click", () => selectTab("word"));
$("#tab-refs").addEventListener("click", () => selectTab("refs"));
$("#tab-notes").addEventListener("click", () => selectTab("notes"));
$("#tab-lists").addEventListener("click", () => selectTab("lists"));
$(".tabs").addEventListener("keydown", (e) => {
  if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
  const cur = TABS.findIndex((t) => $(`#tab-${t}`).getAttribute("aria-selected") === "true");
  const next = TABS[(cur + (e.key === "ArrowRight" ? 1 : TABS.length - 1)) % TABS.length];
  selectTab(next);
  $(`#tab-${next}`).focus();
});
$("#panel-wide").addEventListener("click", () => setWide(!main.classList.contains("wide")));
function setWide(on) {
  main.classList.toggle("wide", on);
  $("#panel-wide").setAttribute("aria-pressed", String(on));
  store.set("wide", on);
}

/* ── navigation ── */
function goTo(book, chapter, verse) {
  S.book = book;
  S.chapter = chapter;
  $("#nav").hidePopover?.();
  return loadChapter(verse);
}
/** Go to a reference given by book code, converting from another version's numbering if needed. */
async function goToCode(code, chapter, verse, fromVersion) {
  const r = await invoke("convert", { from: fromVersion, to: S.v1, code, chapter, verse });
  if (r) return goTo(r[0], r[1], r[2]);
  const bi = S.books.findIndex((b) => b.code === code);
  if (bi >= 0) return goTo(bi, 1);
  empty404(code);
}
function empty404(code) {
  reader.replaceChildren(el("div", { className: "empty" }, el("h2", { textContent: "Not in this version" }), el("p", { textContent: `${S.v1.toUpperCase()} doesn’t contain ${code}. Pick another version to read it.` })));
}
const step = (dir) => { const n = S.data?.[dir]; if (n) goTo(n[0], n[1]); };
$("#prev").addEventListener("click", () => step("prev"));
$("#next").addEventListener("click", () => step("next"));

function testament(code) {
  return OT_CODES.has(code) ? "Old Testament" : NT_CODES.has(code) ? "New Testament" : "Apocrypha & other";
}
function buildNav() {
  const list = $("#nav-books");
  list.replaceChildren();
  S.books.forEach((b, i) => {
    if (i === 0 || testament(b.code) !== testament(S.books[i - 1].code)) list.append(el("h3", { textContent: testament(b.code) }));
    const btn = el("button", { type: "button", textContent: b.name });
    btn.dataset.i = i;
    list.append(btn);
  });
}
$("#nav-books").addEventListener("click", (e) => {
  const b = e.target.closest("button[data-i]");
  if (b) { S.navBook = +b.dataset.i; renderNavChapters(); }
});
$("#nav-chapters").addEventListener("click", (e) => {
  const b = e.target.closest("button[data-c]");
  if (b) goTo(S.navBook, +b.dataset.c);
});
function renderNavChapters() {
  $("#nav-books").querySelectorAll("button").forEach((b) => {
    if (+b.dataset.i === S.navBook) b.setAttribute("aria-current", "true");
    else b.removeAttribute("aria-current");
  });
  $("#nav-book-name").textContent = bookName(S.navBook);
  $("#nav-chapters").replaceChildren(...Array.from({ length: S.books[S.navBook].chapters }, (_, k) => {
    const b = el("button", { type: "button", textContent: k + 1, ariaLabel: `Chapter ${k + 1}` });
    b.dataset.c = k + 1;
    if (S.navBook === S.book && k + 1 === S.chapter) b.setAttribute("aria-current", "true");
    return b;
  }));
}
$("#nav").addEventListener("toggle", (e) => {
  $("#nav-btn").setAttribute("aria-expanded", String(e.newState === "open"));
  if (e.newState !== "open") return;
  S.navBook = S.book;
  renderNavChapters();
  $("#nav-books").querySelector("[aria-current]")?.scrollIntoView({ block: "center" });
});

/* ── command palette ── */
const dlg = $("#palette"), q = $("#q"), results = $("#results");
let items = [], active = 0, pending = 0;
const ICON = {
  go: '<svg class="ico" viewBox="0 0 16 16" aria-hidden="true"><path d="M3 2.5h7a2 2 0 0 1 2 2v9H5a2 2 0 0 1-2-2z"/><path d="M5.5 13.5v-3"/></svg>',
  search: '<svg class="ico" viewBox="0 0 16 16" aria-hidden="true"><circle cx="7" cy="7" r="4.5"/><path d="m10.5 10.5 3 3"/></svg>',
};
function drawResults() {
  results.replaceChildren(...items.map((it, i) => {
    const li = el("li", { role: "option", id: `opt-${i}` });
    li.innerHTML = ICON[it.kind]; // static markup above; user text goes in via textContent
    li.append(el("span", { textContent: it.label }), el("small", { textContent: it.note ?? "" }));
    li.setAttribute("aria-selected", String(i === active));
    li.addEventListener("mousemove", () => { if (active !== i) { active = i; drawResults(); } });
    li.addEventListener("click", () => run(it));
    return li;
  }));
  q.setAttribute("aria-activedescendant", items.length ? `opt-${active}` : "");
}
async function refresh() {
  const text = q.value.trim(), mine = ++pending;
  const next = [];
  if (!text) {
    store.get("history", []).forEach((h) => next.push({ kind: "search", query: h, label: h, note: "Recent" }));
  } else {
    const ref = /["*@]|^[hg]\d|^[/'.]/i.test(text) ? null : await invoke("goto", { version: S.v1, text });
    if (mine !== pending) return;
    if (ref) {
      const verse = /[:.]\s*\d+\s*$/.test(text) ? ref[2] : null;
      next.push({ kind: "go", ref, verse, label: `Go to ${label(ref[0], ref[1], verse)}` });
    }
    next.push({ kind: "search", query: text, label: `Search ${S.v1.toUpperCase()} for “${text}”` });
  }
  items = next;
  active = 0;
  drawResults();
}
function run(it) {
  dlg.close();
  if (it.kind === "go") goTo(it.ref[0], it.ref[1], it.verse);
  else runSearch(it.query);
}
const KEYS = { greek: [..."αβγδεζηθικλμνξοπρστυφχψω"], hebrew: [..."אבגדהוזחטיכלמנסעפצקרשת"] };
const HINTS = {
  latin: ["love*", "wom?n", "\"so loved\"", "G25", "H7225", "/ faith hope"],
  greek: ["λόγος", "λογ*", "\"ἐν ἀρχῇ\"", "ἀγαπάω@v*", "G25"],
  hebrew: ["בראשית", "\"בראשית ברא\"", "ברא@vq*", "H7225"],
};
function drawPalette() {
  const sc = scriptOf(S.v1);
  const hint = $(".hint-syntax");
  hint.replaceChildren("Syntax: ", ...HINTS[sc].flatMap((t) => [el("code", { textContent: t, lang: LANG[sc] }), " "]));
  const pad = $("#keypad"), btn = $("#keypad-btn");
  btn.hidden = sc === "latin";
  pad.hidden = sc === "latin" || btn.getAttribute("aria-expanded") !== "true";
  if (sc === "latin") return;
  pad.dir = sc === "hebrew" ? "rtl" : "ltr";
  pad.replaceChildren(...[...KEYS[sc], "*", "?", "@"].map((ch) => {
    const b = el("button", { type: "button", className: "key", textContent: ch, ariaLabel: `Insert ${ch}`, lang: LANG[sc] });
    b.addEventListener("mousedown", (e) => e.preventDefault()); // keep focus (and the caret) in the search box
    b.addEventListener("click", () => { q.setRangeText(ch, q.selectionStart ?? q.value.length, q.selectionEnd ?? q.value.length, "end"); q.dispatchEvent(new Event("input")); });
    return b;
  }));
}
$("#keypad-btn").addEventListener("click", () => {
  const open = $("#keypad-btn").getAttribute("aria-expanded") !== "true";
  $("#keypad-btn").setAttribute("aria-expanded", String(open));
  drawPalette();
  q.focus();
});
function openPalette() { q.value = ""; drawPalette(); dlg.showModal(); refresh(); }
q.addEventListener("input", refresh);
q.addEventListener("keydown", (e) => {
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    if (items.length) { active = (active + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length; drawResults(); results.children[active]?.scrollIntoView({ block: "nearest" }); }
  } else if (e.key === "Enter") {
    e.preventDefault();
    if (items[active]) run(items[active]);
  }
});
dlg.addEventListener("click", (e) => { if (e.target === dlg) dlg.close(); });
$("#palette-btn").addEventListener("click", openPalette);

document.addEventListener("keydown", (e) => {
  const typing = /^(INPUT|SELECT|TEXTAREA)$/.test(document.activeElement?.tagName);
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") { e.preventDefault(); openPalette(); }
  else if (!typing && !dlg.open && e.key === "/") { e.preventDefault(); openPalette(); }
  else if (!typing && !dlg.open && e.key === "[") step("prev");
  else if (!typing && !dlg.open && e.key === "]") step("next");
});

/* ── versions ── */
function fillVersions() {
  const opt = (v) => el("option", { value: v.id, textContent: LABELS[v.id] ? `${v.id.toUpperCase()} · ${LABELS[v.id]}` : v.id.toUpperCase() });
  const groups = () => Object.entries(GROUPS).map(([sc, name]) => {
    const vs = S.versions.filter((v) => v.script === sc);
    return vs.length ? el("optgroup", { label: name }, ...vs.map(opt)) : null;
  }).filter(Boolean);
  $("#v1").replaceChildren(...groups());
  $("#v2").replaceChildren(el("option", { value: "", textContent: "Compare…" }), ...groups());
  $("#v1").value = S.v1;
  $("#v2").value = S.v2;
}
async function loadBooks(version) {
  S.books = await invoke("books", { version });
  buildNav();
}
async function switchPrimary(id) {
  const old = S.v1, code = S.books[S.book]?.code, verse = S.sel?.verse ?? 1;
  const r = code ? await invoke("convert", { from: old, to: id, code, chapter: S.chapter, verse }) : null;
  S.v1 = id;
  if (S.v2 === id) { S.v2 = ""; $("#v2").value = ""; }
  await loadBooks(id);
  if (r) { S.book = r[0]; S.chapter = r[1]; await loadChapter(S.sel ? r[2] : undefined); return; }
  const bi = S.books.findIndex((b) => b.code === code);
  S.book = bi >= 0 ? bi : 0;
  S.chapter = 1;
  await loadChapter();
}
$("#v1").addEventListener("change", (e) => switchPrimary(e.target.value));
$("#v2").addEventListener("change", (e) => { S.v2 = e.target.value; loadChapter(S.sel?.verse); });
$("#cant-btn").addEventListener("click", () => { S.cant = !S.cant; loadChapter(S.sel?.verse); });

function empty(title, ...body) {
  reader.replaceChildren(el("div", { className: "empty" }, el("h2", { textContent: title }), ...body.map((t) => el("p", { textContent: t }))));
  document.querySelectorAll(".bar button, .bar select").forEach((n) => (n.disabled = true));
}

/* ── start ── */
(async function init() {
  const st = await invoke("status");
  if (!st.dir) return empty("BibleWorks 8 not found", "Bible Study reads the modules from your own BibleWorks 8 install and never copies them.", "It looked in C:\\Program Files (x86)\\BibleWorks 8. If yours is elsewhere, start Bible Study with the BW8_DIR environment variable set to that folder.");
  if (!st.versions.length) return empty("No Bible versions found", `Nothing readable in ${st.dir}\\databases.`);
  S.versions = st.versions;
  const has = (id) => S.versions.some((v) => v.id === id);
  const saved = store.get("state", {});
  S.v1 = has(saved.v1) ? saved.v1 : has("kjv") ? "kjv" : S.versions[0].id;
  S.v2 = has(saved.v2) && saved.v2 !== S.v1 ? saved.v2 : "";
  S.cant = !!saved.cant;
  if (S.versions.some((v) => v.script === "hebrew")) loadFont("hebrew", "BS Hebrew");
  if (S.versions.some((v) => v.script === "greek")) loadFont("greek", "BS Greek");
  fillVersions();
  await loadBooks(S.v1);
  const bi = S.books.findIndex((b) => b.code === (saved.code ?? "Joh"));
  S.book = Math.max(bi, 0);
  const wanted = saved.code ? saved.chapter : 3; // first run opens John 3
  S.chapter = bi >= 0 && wanted >= 1 && wanted <= S.books[bi].chapters ? wanted : 1;
  selectTab("search");
  setWide(store.get("wide", false));
  renderWord();
  $("#pane-search").replaceChildren(el("div", { className: "panel-empty" }, el("strong", { textContent: "Search results appear here" }), "Press Ctrl K, then type a word, a “phrase”, or a Strong’s number like G25."));
  await loadChapter();
})();
