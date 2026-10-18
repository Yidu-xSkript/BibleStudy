"use strict";
/* The study tools that need the whole window: flashcards, timeline (and maps, diagrams).
   Loaded after library.js; shares its globals (el, $, S, invoke, store, goToCode, segmented, note, fail). */

const STUDIO = { tabs: [], current: store.get("studioTab", "cards"), cleanup: null };
const studio = $("#studio"), stBody = $("#st-body");

function openStudio(tab) {
  if (tab) STUDIO.current = tab;
  if (!STUDIO.tabs.some((t) => t.id === STUDIO.current)) STUDIO.current = STUDIO.tabs[0].id;
  document.body.classList.add("studio-open");
  studio.hidden = false;
  $("#studio-btn").setAttribute("aria-pressed", "true");
  drawStudio();
}

function closeStudio() {
  STUDIO.cleanup?.();
  STUDIO.cleanup = null;
  document.body.classList.remove("studio-open");
  studio.hidden = true;
  $("#studio-btn").setAttribute("aria-pressed", "false");
  reader.focus({ preventScroll: true });
}

function drawStudio() {
  STUDIO.cleanup?.();
  STUDIO.cleanup = null;
  const nav = $("#st-tabs");
  nav.replaceChildren(...STUDIO.tabs.map((t) => {
    const b = el("button", { role: "tab", id: `st-tab-${t.id}`, textContent: t.label, tabIndex: t.id === STUDIO.current ? 0 : -1 });
    b.setAttribute("aria-selected", String(t.id === STUDIO.current));
    b.addEventListener("click", () => { STUDIO.current = t.id; store.set("studioTab", t.id); drawStudio(); });
    b.addEventListener("keydown", (e) => {
      const i = STUDIO.tabs.findIndex((x) => x.id === t.id), d = { ArrowRight: 1, ArrowLeft: -1 }[e.key];
      if (!d) return;
      const next = STUDIO.tabs[(i + d + STUDIO.tabs.length) % STUDIO.tabs.length];
      STUDIO.current = next.id;
      store.set("studioTab", next.id);
      drawStudio();
      $(`#st-tab-${next.id}`).focus();
    });
    return b;
  }));
  stBody.className = "st-body";
  stBody.replaceChildren();
  const tab = STUDIO.tabs.find((t) => t.id === STUDIO.current);
  stBody.setAttribute("aria-labelledby", `st-tab-${tab.id}`);
  tab.render(stBody);
}

$("#studio-btn").addEventListener("click", () => (studio.hidden ? openStudio() : closeStudio()));
$("#st-close").addEventListener("click", closeStudio);
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !studio.hidden && !document.querySelector("dialog[open]") && !e.defaultPrevented) closeStudio();
});

const plural = (n, one, many = one + "s") => `${n.toLocaleString()} ${n === 1 ? one : many}`;
const fieldRow = (label, input) => el("label", { className: "st-field" }, el("span", { textContent: label }), input);
const numInput = (value, props = {}) => el("input", { type: "number", min: 0, step: 1, value: value ?? "", className: "field", ...props });

/* ═════════════════════════════ flashcards ═════════════════════════════ */
const FC = {
  decks: null,
  deck: store.get("fcDeck", null),
  filter: store.get("fcFilter", { min_lesson: null, max_lesson: null, min_count: null }),
  opts: store.get("fcOpts", { reverse: false, shuffle: true, size: 20 }),
  open: null, // the opened deck: {title, hebrew, cards, queue, stats}
  view: "setup", // setup | browse | run | done
  run: null,
  sort: "deck",
  find: "",
};
const levelPips = (level) => el("span", { className: "pips", title: level == null ? "New" : `Box ${level + 1} of 6`, role: "img", ariaLabel: level == null ? "New card" : `Box ${level + 1} of 6` },
  ...Array.from({ length: 5 }, (_, i) => el("i", { className: level != null && level >= i + 1 ? "on" : "" })));

function toFilter() {
  const f = FC.filter, n = (v) => (v === "" || v == null || Number.isNaN(+v) ? null : Math.max(0, +v));
  return { min_lesson: n(f.min_lesson), max_lesson: n(f.max_lesson), min_count: n(f.min_count) };
}

async function fcLoad() {
  FC.open = FC.deck ? await invoke("fc_open", { deck: FC.deck, filter: toFilter(), limit: FC.opts.size || 100000 }) : null;
}

async function renderCards(body) {
  body.className = "st-body st-cards";
  body.append(note("Loading decks…"));
  try {
    FC.decks ??= await invoke("fc_decks");
    if (!FC.decks.length) return body.replaceChildren(el("div", { className: "empty" }, el("h2", { textContent: "No flashcard decks found" }), el("p", { textContent: "Decks come from the vocabulary files in your BibleWorks install (the init folder)." })));
    if (!FC.decks.some((d) => d.id === FC.deck)) FC.deck = FC.decks.find((d) => d.id === "gntvoc")?.id ?? FC.decks[0].id;
    await fcLoad();
  } catch (e) { return body.replaceChildren(fail(e)); }
  if (FC.view === "run" && !FC.run) FC.view = "setup";
  drawCards(body);
}

let fcKey = null;
function fcDetach() { if (fcKey) document.removeEventListener("keydown", fcKey, true); fcKey = null; }

function drawCards(body) {
  fcDetach();
  STUDIO.cleanup = fcDetach;
  const draw = { setup: fcSetup, browse: fcBrowse, run: fcRun, done: fcDone }[FC.view];
  body.replaceChildren();
  draw(body);
}

function fcSetup(body) {
  const info = FC.decks.find((d) => d.id === FC.deck), o = FC.open, st = o.stats;
  const groups = [["Greek", false], ["Hebrew", true]];
  const sel = el("select", { className: "field", ariaLabel: "Deck" });
  for (const [name, heb] of groups) {
    const og = el("optgroup", { label: name });
    FC.decks.filter((d) => d.hebrew === heb).forEach((d) => og.append(el("option", { value: d.id, textContent: `${d.title}${FC.decks.filter((x) => x.title === d.title).length > 1 ? ` [${d.id}]` : ""} (${d.cards.toLocaleString()})`, selected: d.id === FC.deck })));
    if (og.children.length) sel.append(og);
  }
  sel.addEventListener("change", async () => { FC.deck = sel.value; store.set("fcDeck", FC.deck); FC.filter = { min_lesson: null, max_lesson: null, min_count: null }; await fcLoad(); drawCards(body); });

  const refresh = async () => { store.set("fcFilter", FC.filter); store.set("fcOpts", FC.opts); await fcLoad(); drawCards(body); };
  const lessons = info.lessons > 0;
  const lo = numInput(FC.filter.min_lesson, { max: info.lessons, placeholder: "1" }), hi = numInput(FC.filter.max_lesson, { max: info.lessons, placeholder: String(info.lessons) });
  const cnt = numInput(FC.filter.min_count, { placeholder: "any" });
  lo.addEventListener("change", () => { FC.filter.min_lesson = lo.value; refresh(); });
  hi.addEventListener("change", () => { FC.filter.max_lesson = hi.value; refresh(); });
  cnt.addEventListener("change", () => { FC.filter.min_count = cnt.value; refresh(); });
  const size = el("select", { className: "field", ariaLabel: "Cards per session" }, ...[10, 20, 50, 100, 0].map((n) => el("option", { value: n, textContent: n ? `${n} cards` : "All", selected: n === (FC.opts.size ?? 20) })));
  size.addEventListener("change", () => { FC.opts.size = +size.value; refresh(); });

  const go = el("button", { className: "btn big", type: "button", textContent: o.queue.length ? `Study ${plural(o.queue.length, "card")}` : "Nothing to study", disabled: !o.queue.length });
  go.addEventListener("click", () => fcStart(body));
  const reset = el("button", { className: "btn ghost small", type: "button", textContent: "Reset progress", disabled: st.new === st.total });
  reset.addEventListener("click", async () => {
    if (reset.dataset.sure !== "1") { reset.dataset.sure = "1"; reset.textContent = "Click again to forget all progress"; return; }
    await invoke("fc_reset", { deck: FC.deck });
    refresh();
  });
  const browse = el("button", { className: "btn ghost", type: "button", textContent: "Browse cards", onclick: () => { FC.view = "browse"; drawCards(body); } });

  body.append(el("div", { className: "st-col" },
    el("h2", { className: "st-title", textContent: "Flashcards" }),
    el("p", { className: "muted", textContent: o.comment ? o.comment.replace(/^Copyright.*$/i, "").trim() || "Vocabulary from your BibleWorks install." : "Vocabulary from your BibleWorks install." }),
    fieldRow("Deck", sel),
    lessons ? el("div", { className: "st-pair" }, fieldRow("From lesson", lo), fieldRow("To lesson", hi)) : "",
    fieldRow("Occurring at least", cnt),
    fieldRow("Session", size),
    el("div", { className: "st-opts" },
      checkbox("Shuffle", FC.opts.shuffle, (v) => { FC.opts.shuffle = v; store.set("fcOpts", FC.opts); }),
      checkbox("Meaning first", FC.opts.reverse, (v) => { FC.opts.reverse = v; store.set("fcOpts", FC.opts); })),
    el("dl", { className: "st-stats" },
      el("div", {}, el("dt", { textContent: "New" }), el("dd", { textContent: st.new.toLocaleString() })),
      el("div", {}, el("dt", { textContent: "Due" }), el("dd", { textContent: st.due.toLocaleString() })),
      el("div", {}, el("dt", { textContent: "Learned" }), el("dd", { textContent: `${st.learned.toLocaleString()} of ${st.total.toLocaleString()}` }))),
    el("div", { className: "st-actions" }, go, browse, reset)));
}

function checkbox(text, checked, onChange) {
  const i = el("input", { type: "checkbox", checked });
  i.addEventListener("change", () => onChange(i.checked));
  return el("label", { className: "st-check" }, i, el("span", { textContent: text }));
}

function shuffle(a) {
  for (let i = a.length - 1; i > 0; i--) { const j = Math.floor(Math.random() * (i + 1)); [a[i], a[j]] = [a[j], a[i]]; }
  return a;
}

function fcStart(body) {
  const o = FC.open;
  const order = FC.opts.shuffle ? shuffle([...o.queue]) : [...o.queue];
  FC.run = { order, pos: 0, flipped: false, right: 0, missed: [], total: order.length, learned: new Set() };
  FC.view = "run";
  drawCards(body);
}

function cardFace(card, text, hebrew, big) {
  const word = text === card.front;
  return el("div", { className: "fc-text " + (word ? (hebrew ? "hebrew" : "greek") + " fc-word" : "fc-gloss"), lang: word ? (hebrew ? "he" : "grc") : "en", dir: word && hebrew ? "rtl" : "ltr" },
    ...text.split("\n").map((l) => el("div", { textContent: l })));
}

function fcRun(body) {
  const r = FC.run, o = FC.open;
  if (r.pos >= r.order.length) { FC.view = "done"; return drawCards(body); }
  const card = o.cards[r.order[r.pos]];
  const [first, second] = FC.opts.reverse ? [card.back, card.front] : [card.front, card.back];
  const flipped = r.flipped;
  const face = el("div", { className: "fc-card" + (flipped ? " flipped" : ""), role: "group", ariaLabel: "Card" },
    cardFace(card, first, o.hebrew),
    flipped ? el("hr", { className: "fc-rule" }) : "",
    flipped ? cardFace(card, second, o.hebrew) : "",
    flipped ? el("p", { className: "fc-meta muted", textContent: [card.lesson ? `Lesson ${card.lesson}` : "", card.count ? `${plural(card.count, "occurrence")}` : ""].filter(Boolean).join(" · ") }) : "");
  const learned = r.learned.has(card.id) || (card.level ?? 0) >= 3;
  const bar = el("div", { className: "fc-progress", role: "progressbar", ariaValueMin: 0, ariaValueMax: r.order.length, ariaValueNow: r.pos, ariaLabel: "Session progress" }, el("i", { style: `width:${(r.pos / r.order.length) * 100}%` }));
  const controls = el("div", { className: "fc-controls" });
  if (!flipped) {
    controls.append(el("button", { className: "btn big", type: "button", textContent: "Show answer", onclick: () => flip() }), el("kbd", { textContent: "Space" }));
  } else {
    controls.append(
      el("button", { className: "btn ghost big", type: "button", onclick: () => answer(false) }, "Missed ", el("kbd", { textContent: "1" })),
      el("button", { className: "btn big", type: "button", onclick: () => answer(true) }, "Got it ", el("kbd", { textContent: "2" })),
      el("button", { className: "btn ghost", type: "button", ariaPressed: String(learned), onclick: () => markLearned(card, !learned) }, learned ? "Learned ✓" : "Mark learned", " ", el("kbd", { textContent: "L" })));
  }
  const end = el("button", { className: "btn ghost small", type: "button", textContent: "End session", onclick: () => stop() });
  body.append(el("div", { className: "st-col fc" }, el("div", { className: "fc-top" }, el("span", { className: "muted", textContent: `${r.pos + 1} of ${r.order.length}` }), end), bar, face, controls));
  face.addEventListener("click", () => { if (!FC.run.flipped) flip(); });

  function flip() { r.flipped = true; drawCards(body); }
  async function answer(right) {
    r.flipped = false;
    if (right) r.right++; else { r.missed.push(card); r.order.push(r.order[r.pos]); }
    r.pos++;
    drawCards(body);
    await invoke("fc_grade", { deck: FC.deck, card: card.id, right }).catch(() => {});
  }
  async function markLearned(c, on) {
    on ? r.learned.add(c.id) : r.learned.delete(c.id);
    c.level = on ? 5 : 0;
    drawCards(body);
    await invoke("fc_learned", { deck: FC.deck, card: c.id, learned: on }).catch(() => {});
  }
  function stop() { FC.view = "setup"; FC.run = null; renderCards(body); }
  function onKey(e) {
    if (e.ctrlKey || e.metaKey || e.altKey || e.target.closest?.("input, select, textarea")) return;
    const k = e.key;
    if (k === "Escape") { e.preventDefault(); e.stopImmediatePropagation(); stop(); }
    else if (!r.flipped && (k === " " || k === "Enter")) { e.preventDefault(); flip(); }
    else if (r.flipped && (k === "1" || k === "ArrowLeft")) { e.preventDefault(); answer(false); }
    else if (r.flipped && (k === "2" || k === "ArrowRight" || k === "Enter" || k === " ")) { e.preventDefault(); answer(true); }
    else if (r.flipped && (k === "l" || k === "L")) { e.preventDefault(); markLearned(card, !learned); }
  }
  fcKey = onKey;
  document.addEventListener("keydown", onKey, true);
  controls.querySelector(".btn:not(.ghost)")?.focus({ preventScroll: true });
}

function fcDone(body) {
  const r = FC.run;
  const again = el("button", { className: "btn big", type: "button", textContent: "Back to the deck", onclick: () => { FC.view = "setup"; FC.run = null; renderCards(body); } });
  body.append(el("div", { className: "st-col" },
    el("h2", { className: "st-title", textContent: "Session complete" }),
    el("p", { className: "fc-score" }, el("b", { textContent: `${r.right} of ${r.total}` }), " cards right the first time through."),
    r.missed.length ? el("section", {}, el("h3", { className: "st-sub", textContent: "Missed" }),
      el("ul", { className: "fc-missed" }, ...[...new Map(r.missed.map((c) => [c.id, c])).values()].map((c) => el("li", {},
        el("span", { className: FC.open.hebrew ? "hebrew" : "greek", lang: FC.open.hebrew ? "he" : "grc", dir: FC.open.hebrew ? "rtl" : "ltr", textContent: c.front }),
        el("span", { className: "muted", textContent: c.back.replace(/\n/g, " · ") }))))) : el("p", { className: "muted", textContent: "No misses. Well done." }),
    el("div", { className: "st-actions" }, again)));
  again.focus({ preventScroll: true });
}

function fcBrowse(body) {
  const o = FC.open;
  const q = FC.find.trim().toLowerCase();
  const hay = (c) => (c.front + " " + c.back).toLowerCase();
  let rows = o.cards.filter((c) => {
    const f = toFilter();
    return (!q || hay(c).includes(q)) && (f.min_lesson == null || (c.lesson ?? 0) >= f.min_lesson) && (f.max_lesson == null || (c.lesson ?? 0) <= f.max_lesson) && (f.min_count == null || c.count >= f.min_count);
  });
  const sorts = { deck: null, count: (a, b) => b.count - a.count, lesson: (a, b) => (a.lesson ?? 0) - (b.lesson ?? 0) || b.count - a.count, level: (a, b) => (a.level ?? -1) - (b.level ?? -1), alpha: (a, b) => a.back.localeCompare(b.back, "en") };
  if (sorts[FC.sort]) rows = [...rows].sort(sorts[FC.sort]);
  const find = el("input", { type: "search", className: "field", placeholder: "Find a word or meaning", value: FC.find, ariaLabel: "Find in deck" });
  const sort = el("select", { className: "field", ariaLabel: "Sort" }, ...[["deck", "Deck order"], ["count", "Most frequent"], ["lesson", "Lesson"], ["level", "Least learned"], ["alpha", "Meaning A–Z"]].map(([v, t]) => el("option", { value: v, textContent: t, selected: v === FC.sort })));
  const list = el("ul", { className: "fc-list" });
  const limit = 400;
  const draw = () => list.replaceChildren(...rows.slice(0, limit).map((c) => el("li", {},
    el("span", { className: "fc-w " + (o.hebrew ? "hebrew" : "greek"), lang: o.hebrew ? "he" : "grc", dir: o.hebrew ? "rtl" : "ltr", textContent: c.front }),
    el("span", { className: "fc-g", textContent: c.back.replace(/\n/g, " · ") }),
    el("span", { className: "fc-n muted", textContent: c.count ? c.count.toLocaleString() : "" }),
    levelPips(c.level))));
  draw();
  find.addEventListener("input", () => { FC.find = find.value; drawCards(body); find.focus(); find.setSelectionRange(find.value.length, find.value.length); });
  sort.addEventListener("change", () => { FC.sort = sort.value; drawCards(body); });
  body.append(el("div", { className: "st-col wide" },
    el("div", { className: "fc-top" }, el("h2", { className: "st-title", textContent: o.title }), el("button", { className: "btn ghost small", type: "button", textContent: "Back", onclick: () => { FC.view = "setup"; drawCards(body); } })),
    el("div", { className: "st-pair" }, find, sort),
    el("p", { className: "sum", textContent: `${plural(rows.length, "card")}${rows.length > limit ? `, showing the first ${limit}` : ""}` }),
    list));
}

STUDIO.tabs.push({ id: "cards", label: "Flashcards", render: renderCards });

/* ═════════════════════════════ timeline ═════════════════════════════ */
const TL = { all: null, idx: store.get("tlIdx", 0), zoom: 1, sel: null, find: "" };
const TL_NAMES = { BIBLE: "Bible history", NT_DET: "New Testament in detail", WORLD: "World history" };
const SVG = "http://www.w3.org/2000/svg";
const svg = (tag, attrs = {}, ...kids) => {
  const n = document.createElementNS(SVG, tag);
  for (const [k, v] of Object.entries(attrs)) n.setAttribute(k, v);
  n.append(...kids);
  return n;
};
const astro = (d) => (d < 0 ? d + 1 : d);
const yearText = (d) => (d < 0 ? `${-d} BC` : d === 0 ? "1 BC / AD 1" : `AD ${d}`);
const spanText = (e) => (e.end === e.start ? yearText(e.start) : `${yearText(e.start)} – ${yearText(e.end)}`) + (e.uncertain ? " (approximate)" : "");

/** BibleWorks colour index -> a hue for a calm, theme-aware mark (greys stay neutral). */
function hueOf(rgb) {
  const [r, g, b] = rgb.map((x) => x / 255), mx = Math.max(r, g, b), mn = Math.min(r, g, b), d = mx - mn;
  if (d < 0.18) return null;
  const h = mx === r ? ((g - b) / d) % 6 : mx === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return Math.round(((h * 60) + 360) % 360);
}
const EGA = [[0, 0, 0], [0, 0, 170], [0, 170, 0], [0, 170, 170], [170, 0, 0], [170, 0, 170], [170, 85, 0], [170, 170, 170], [85, 85, 85], [85, 85, 255], [85, 255, 85], [85, 255, 255], [255, 85, 85], [255, 85, 255], [255, 255, 85], [255, 255, 255]];
const tone = (idx) => { const h = hueOf(EGA[idx % 16]); return h == null ? "var(--muted)" : `oklch(var(--tl-l) var(--tl-c) ${h})`; };

async function renderTimeline(body) {
  body.className = "st-body st-timeline";
  body.append(note("Loading timelines…"));
  try { TL.all ??= await invoke("tl_all"); } catch (e) { return body.replaceChildren(fail(e)); }
  if (!TL.all.length) return body.replaceChildren(el("div", { className: "empty" }, el("h2", { textContent: "No timelines found" }), el("p", { textContent: "Timelines come from the timeline folder of your BibleWorks install." })));
  TL.idx = Math.min(TL.idx, TL.all.length - 1);
  drawTimeline(body);
}

function drawTimeline(body) {
  const tl = TL.all[TL.idx];
  body.replaceChildren();
  const pick = el("select", { className: "field", ariaLabel: "Timeline" }, ...TL.all.map((t, i) => el("option", { value: i, textContent: TL_NAMES[t.name] ?? t.name, selected: i === TL.idx })));
  pick.addEventListener("change", () => { TL.idx = +pick.value; TL.sel = null; TL.find = ""; TL.zoom = 1; store.set("tlIdx", TL.idx); drawTimeline(body); });
  const find = el("input", { type: "search", className: "field", placeholder: "Find an event", ariaLabel: "Find an event", value: TL.find });
  const found = el("div", { className: "tl-found", hidden: true });
  const zoomBtn = (text, label, f) => el("button", { className: "icon-btn", type: "button", textContent: text, title: label, ariaLabel: label, onclick: () => { TL.zoom = Math.min(8, Math.max(0.25, TL.zoom * f)); layout(); } });
  const bar = el("div", { className: "tl-bar" }, pick, find, el("div", { className: "grow" }), zoomBtn("−", "Zoom out", 1 / 1.4), el("span", { className: "muted tl-zoom", id: "tl-zoom" }), zoomBtn("+", "Zoom in", 1.4));
  const scroll = el("div", { className: "tl-scroll", tabIndex: 0, role: "region", ariaLabel: `${TL_NAMES[tl.name] ?? tl.name} timeline` });
  const detail = el("aside", { className: "tl-detail", "aria-live": "polite" });
  body.append(bar, found, el("div", { className: "tl-main" }, scroll, detail));

  const A0 = astro(tl.axis.first), A1 = astro(tl.axis.last);
  const rowsOf = (e) => tl.eras[e.era].y + e.y;
  const ys = tl.events.map(rowsOf), minY = Math.min(0, ...ys), maxY = Math.max(...ys, 0);
  const YS = 3, PAD = 28;
  let scale = 1;

  function layout() {
    const base = Math.max(3200, scroll.clientWidth * 2) / Math.max(1, A1 - A0);
    scale = base * TL.zoom;
    $("#tl-zoom").textContent = `${Math.round(TL.zoom * 100)}%`;
    const w = Math.ceil((A1 - A0) * scale) + 240, h = Math.ceil((maxY - minY) * YS) + PAD * 2 + 30;
    const X = (d) => (astro(d) - A0) * scale + 24;
    const Y = (e) => (rowsOf(e) - minY) * YS + PAD;

    const axis = svg("svg", { class: "tl-axis", width: w, height: 28, "aria-hidden": "true" });
    const steps = [1, 2, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000, 2000];
    const step = steps.find((s) => s * scale >= 80) ?? 2000;
    const grid = svg("g", { class: "tl-grid" });
    for (let bw = Math.ceil(tl.axis.first / step) * step; bw <= tl.axis.last; bw += step) {
      if (bw === 0) continue;
      const x = (astro(bw) - A0) * scale + 24;
      axis.append(svg("line", { x1: x, x2: x, y1: 20, y2: 28, class: "tl-tick" }), svg("text", { x: x + 4, y: 16, class: "tl-year" }, bw < 0 ? `${-bw} BC` : `AD ${bw}`));
      grid.append(svg("line", { x1: x, x2: x, y1: 0, y2: h }));
    }
    const canvas = svg("svg", { class: "tl-canvas", width: w, height: h, role: "list" }, grid);
    const need = (TL.find || "").trim().toLowerCase();
    tl.events.forEach((e, i) => {
      const x1 = X(e.start), x2 = Math.max(X(e.end), x1), y = Y(e);
      const bar = e.style === "EraBar";
      const g = svg("g", { class: "tl-ev" + (i === TL.sel ? " sel" : "") + (need && e.name.toLowerCase().includes(need) ? " hit" : ""), "data-i": i, role: "listitem", tabindex: -1 });
      g.style.setProperty("--c", tone(e.extent_color));
      if (bar) g.append(svg("rect", { x: x1, y: y - 7, width: Math.max(x2 - x1, 2), height: 14, rx: 3, class: "tl-bar-shape" }));
      else if (x2 - x1 > 3) g.append(svg("line", { x1, x2, y1: y, y2: y, class: "tl-line" }), svg("line", { x1, x2: x1, y1: y - 3, y2: y + 3, class: "tl-line" }), svg("line", { x1: x2, x2, y1: y - 3, y2: y + 3, class: "tl-line" }));
      else g.append(svg("circle", { cx: x1, cy: y, r: 3.5, class: "tl-dot" }));
      g.append(svg("text", { x: x1 + (bar ? 6 : x2 - x1 > 3 ? 0 : 8), y: bar ? y + 4 : y - 6, class: "tl-cap" + (bar ? " on-bar" : "") }, e.name));
      g.append(svg("rect", { x: x1 - 4, y: y - 12, width: Math.max(x2 - x1, 8) + 8, height: 20, class: "tl-hit" }));
      canvas.append(g);
    });
    scroll.replaceChildren(axis, canvas);
    scroll.dataset.ready = "1";
  }

  function select(i, center = false) {
    TL.sel = i;
    scroll.querySelectorAll(".tl-ev.sel").forEach((n) => n.classList.remove("sel"));
    const g = scroll.querySelector(`.tl-ev[data-i="${i}"]`);
    g?.classList.add("sel");
    if (g && center) {
      const r = g.getBBox();
      scroll.scrollTo({ left: Math.max(0, r.x - scroll.clientWidth / 3), top: Math.max(0, r.y - scroll.clientHeight / 2), behavior: "smooth" });
    }
    showDetail();
  }

  function showDetail() {
    const e = TL.sel == null ? null : tl.events[TL.sel];
    if (!e) return detail.replaceChildren(el("p", { className: "muted", textContent: "Select an event to see its dates and passages." }));
    const era = tl.eras[e.era];
    const refs = e.links.filter((l) => l.Bible || l.Tsk), other = e.links.filter((l) => l.Dictionary || l.Search);
    detail.replaceChildren(
      el("h3", { className: "st-sub", textContent: e.name }),
      el("p", { className: "tl-dates", textContent: spanText(e) }),
      el("p", { className: "muted", textContent: era.name }),
      e.repaired ? el("p", { className: "muted", textContent: "The source file has this date range reversed; it is shown as the likely intent." }) : "",
      refs.length ? el("div", { className: "tl-links" }, ...refs.map((l) => {
        const r = l.Bible ?? l.Tsk;
        const text = `${S.books.find((b) => b.code === r.book)?.name ?? r.book} ${r.chapter}${r.verse ? ":" + r.verse : ""}${r.end_verse ? "–" + r.end_verse : r.end_chapter ? "–" + r.end_chapter : ""}`;
        return el("button", { className: "chip", type: "button", textContent: text, onclick: () => { closeStudio(); goToCode(r.book, r.chapter, r.verse ?? 1, "kjv"); } });
      })) : "",
      other.length ? el("p", { className: "muted tl-other", textContent: "Also: " + other.map((l) => l.Dictionary ? `${l.Dictionary.dict} “${l.Dictionary.term}”` : `search “${l.Search}”`).join("; ") }) : "");
  }

  scroll.addEventListener("click", (ev) => { const g = ev.target.closest?.(".tl-ev"); if (g) select(+g.dataset.i); });
  scroll.addEventListener("wheel", (ev) => {
    if (!ev.ctrlKey) return;
    ev.preventDefault();
    TL.zoom = Math.min(8, Math.max(0.25, TL.zoom * (ev.deltaY < 0 ? 1.15 : 1 / 1.15)));
    layout();
  }, { passive: false });
  find.addEventListener("input", () => {
    TL.find = find.value;
    const q = TL.find.trim().toLowerCase();
    const hits = q ? tl.events.map((e, i) => [e, i]).filter(([e]) => e.name.toLowerCase().includes(q)) : [];
    found.hidden = !hits.length;
    found.replaceChildren(...hits.slice(0, 8).map(([e, i]) => el("button", { className: "chip", type: "button", onclick: () => select(i, true) }, e.name, el("small", { textContent: yearText(e.start) }))));
    scroll.querySelectorAll(".tl-ev").forEach((g) => g.classList.toggle("hit", !!q && tl.events[+g.dataset.i].name.toLowerCase().includes(q)));
  });
  layout();
  showDetail();
  if (TL.sel != null) select(TL.sel, true);
}

STUDIO.tabs.push({ id: "time", label: "Timeline", render: renderTimeline });
