"use strict";
/* Notes, verse lists and word lists. Loaded after app.js and shares its globals (el, S, invoke, goToCode ...). */

/* ── small shared pieces ── */
function download(name, text, type = "text/plain") {
  const a = el("a", { href: URL.createObjectURL(new Blob([text], { type })), download: name });
  document.body.append(a);
  a.click();
  a.remove();
}
const safeName = (s) => (s || "list").replace(/[\\/:*?"<>|]+/g, "-").trim() || "list";

/** One-line form (replaces prompt()): returns an element; call onSubmit(value) on Enter or the button. */
function inlineForm({ placeholder, label, value = "", onSubmit, onCancel }) {
  const input = el("input", { type: "text", placeholder, value, ariaLabel: placeholder, className: "field" });
  const go = () => { if (input.value.trim()) onSubmit(input.value.trim()); };
  const form = el("div", { className: "inline-form" }, input, el("button", { className: "btn small", type: "button", textContent: label, onclick: go }));
  if (onCancel) form.append(el("button", { className: "btn small ghost", type: "button", textContent: "Cancel", onclick: onCancel }));
  input.addEventListener("keydown", (e) => { if (e.key === "Enter") go(); else if (e.key === "Escape" && onCancel) onCancel(); });
  queueMicrotask(() => { input.focus(); input.select(); });
  return form;
}

function segmented(options, current, onPick, label) {
  return el("div", { className: "seg", role: "group", ariaLabel: label }, ...options.map(([id, text]) => {
    const b = el("button", { type: "button", textContent: text, ariaPressed: String(id === current) });
    b.addEventListener("click", () => onPick(id));
    return b;
  }));
}

const note = (text) => el("p", { className: "muted pad", textContent: text });
const fail = (e) => note(String(e));

/* ═════════════════════════════ notes ═════════════════════════════ */
const N = { mode: store.get("noteMode", "verse"), pending: null, timer: null };

function noteTarget() {
  const code = S.books[S.book]?.code;
  if (!code) return null;
  if (N.mode === "chapter") return { code, chapter: S.chapter, verse: 0 };
  return S.sel ? { code, chapter: S.chapter, verse: S.sel.verse } : null;
}

async function flushNote() {
  clearTimeout(N.timer);
  const p = N.pending;
  N.pending = null;
  if (p) await invoke("note_set", { key: p.key, text: p.text });
}

/** Mark verse numbers that have a note (from the chapter response). */
function markNotes() {
  const has = new Set(S.data?.notes ?? []);
  reader.querySelectorAll(".verse").forEach((row) => row.querySelector(".vnum")?.classList.toggle("has-note", has.has(+row.dataset.v)));
}

let notesReq = 0;
async function renderNotes() {
  const pane = $("#pane-notes");
  const mine = ++notesReq;
  await flushNote();
  const target = noteTarget();
  const modes = segmented([["verse", "Verse"], ["chapter", "Chapter"]], N.mode, (m) => { N.mode = m; store.set("noteMode", m); renderNotes(); }, "Note scope");
  const parts = [el("div", { className: "note-head" }, modes)];

  if (!target) {
    parts.push(el("div", { className: "panel-empty" }, el("strong", { textContent: "Select a verse" }), "Click a verse number to write a note on it, or switch to Chapter notes."));
  } else {
    let cur;
    try { cur = await invoke("note_get", { version: S.v1, code: target.code, chapter: target.chapter, verse: target.verse }); }
    catch (e) { pane.replaceChildren(...parts, fail(e)); return; }
    if (mine !== notesReq) return;
    const status = el("span", { className: "muted small", textContent: cur?.updated ? `Saved ${new Date(cur.updated * 1000).toLocaleString()}` : "" });
    const area = el("textarea", { className: "notearea", value: cur?.text ?? "", placeholder: `Write a note on ${cur?.label ?? "this passage"}…`, ariaLabel: `Note on ${cur?.label ?? "this passage"}`, rows: 8 });
    area.addEventListener("input", () => {
      N.pending = { key: cur.key, text: area.value };
      status.textContent = "Saving…";
      clearTimeout(N.timer);
      N.timer = setTimeout(async () => {
        try { await flushNote(); status.textContent = "Saved"; setNoteMarker(target, area.value.trim() !== ""); }
        catch (e) { status.textContent = String(e); }
      }, 600);
    });
    area.addEventListener("blur", () => flushNote().then(() => { if (status.textContent === "Saving…") status.textContent = "Saved"; }).catch(() => {}));
    parts.push(el("h3", { className: "note-title", textContent: cur?.label ?? "" }), area, el("div", { className: "note-foot" }, status));
    $("#tab-notes .dot").hidden = !(cur?.text ?? "").trim();
  }
  parts.push(el("div", { className: "rule" }), await notesBrowser());
  if (mine === notesReq) pane.replaceChildren(...parts);
}

function setNoteMarker(target, on) {
  if (target.verse) reader.querySelector(`.verse[data-v="${target.verse}"] .vnum`)?.classList.toggle("has-note", on);
  $("#tab-notes .dot").hidden = !on;
  if (S.data) S.data.notes = [...new Set([...(S.data.notes ?? []).filter((n) => n !== target.verse), ...(on && target.verse ? [target.verse] : [])])];
}

let notesQuery = "";
async function notesBrowser() {
  const box = el("section", { className: "notes-all" });
  const list = el("div", { className: "note-list" });
  const search = el("input", { type: "search", className: "field", placeholder: "Search all notes", value: notesQuery, ariaLabel: "Search all notes" });
  const fill = async () => {
    let all;
    try { all = await invoke("notes_all", { query: notesQuery }); } catch (e) { list.replaceChildren(fail(e)); return; }
    if (!all.length) { list.replaceChildren(note(notesQuery ? "No notes match." : "No notes yet. Select a verse and start writing.")); return; }
    list.replaceChildren(...all.map((n) => {
      const b = el("button", { className: "note-item", type: "button" }, el("b", { textContent: n.label }), el("span", { textContent: n.text.length > 160 ? n.text.slice(0, 160) + "…" : n.text }));
      b.addEventListener("click", async () => {
        await flushNote();
        N.mode = n.verse ? "verse" : "chapter";
        store.set("noteMode", N.mode);
        goToCode(n.code, n.chapter, n.verse || undefined, n.ns);
      });
      return b;
    }));
  };
  let t;
  search.addEventListener("input", () => { notesQuery = search.value; clearTimeout(t); t = setTimeout(fill, 150); });
  box.append(el("h3", { className: "note-title", textContent: "All notes" }), search, list);
  await fill();
  return box;
}

/** Called by app.js when the chapter or the selected verse changes. */
function notesChanged() {
  if ($("#pane-notes") && !$("#pane-notes").hidden) renderNotes();
  else refreshNoteDot();
}
async function refreshNoteDot() {
  const t = noteTarget();
  if (!t) { $("#tab-notes .dot").hidden = true; return; }
  try {
    const cur = await invoke("note_get", { version: S.v1, code: t.code, chapter: t.chapter, verse: t.verse });
    $("#tab-notes .dot").hidden = !(cur?.text ?? "").trim();
  } catch { /* leave as is */ }
}

/* ═════════════════════════════ lists ═════════════════════════════ */
const L = { mode: store.get("listMode", "verses"), vlist: store.get("vlist", ""), wlist: "", words: null, wsort: "count", wfilter: "", wshown: 300, flash: "" };

function renderLists() {
  const pane = $("#pane-lists");
  const modes = segmented([["verses", "Verses"], ["words", "Words"]], L.mode, (m) => { L.mode = m; store.set("listMode", m); renderLists(); }, "List type");
  const body = el("div", { className: "list-body" });
  pane.replaceChildren(el("div", { className: "note-head" }, modes), body);
  (L.mode === "verses" ? renderVerseLists : renderWordLists)(body);
}

const OPS = [["common", "Verses in both"], ["only_first", "Only in this list"], ["only_second", "Only in the other list"], ["union", "Combine both"]];
const WOPS = [["common", "Words in both"], ["only_first", "Only in the first list"], ["only_second", "Only in the second list"], ["union", "Combine both"]];

/* ── verse lists ── */
async function renderVerseLists(body) {
  let all;
  try { all = await invoke("vl_all"); } catch (e) { body.replaceChildren(fail(e)); return; }
  if (!all.some((l) => l.id === L.vlist)) L.vlist = all[0]?.id ?? "";
  store.set("vlist", L.vlist);
  const reload = () => renderLists();
  const msg = el("p", { className: "muted small pad", role: "status", textContent: L.flash });
  L.flash = "";
  const slot = el("div");

  const select = el("select", { className: "field", ariaLabel: "Verse list" }, ...all.map((l) => el("option", { value: l.id, textContent: `${l.name} (${l.count})` })));
  select.value = L.vlist;
  select.addEventListener("change", () => { L.vlist = select.value; store.set("vlist", L.vlist); reload(); });
  const newBtn = el("button", { className: "btn small", type: "button", textContent: "New" });
  newBtn.addEventListener("click", () => slot.replaceChildren(inlineForm({
    placeholder: "Name of the new list", label: "Create", onCancel: () => slot.replaceChildren(),
    onSubmit: async (name) => { try { L.vlist = await invoke("vl_create", { name, refs: [] }); reload(); } catch (e) { msg.textContent = String(e); } },
  })));
  const importBtn = el("button", { className: "btn small ghost", type: "button", textContent: "Import .vls" });
  const file = el("input", { type: "file", accept: ".vls,.lst,text/plain", hidden: true });
  importBtn.addEventListener("click", () => file.click());
  file.addEventListener("change", async () => {
    const f = file.files[0];
    if (!f) return;
    try { L.vlist = await invoke("vl_import", { text: await f.text(), name: null }); reload(); }
    catch (e) { msg.textContent = String(e); }
  });
  body.replaceChildren(el("div", { className: "toolbar" }, select, newBtn, importBtn, file), slot, msg);
  if (!all.length) { body.append(el("div", { className: "panel-empty" }, el("strong", { textContent: "No verse lists yet" }), "Create one, import a BibleWorks .vls file, or save a search’s results as a list.")); return; }

  let list;
  try { list = await invoke("vl_get", { id: L.vlist }); } catch (e) { body.append(fail(e)); return; }
  if (!list) return;

  // add controls
  const passage = el("input", { type: "text", className: "field", placeholder: "Add a passage, e.g. Eph 2:8-10", ariaLabel: "Add passage" });
  const add = async () => {
    if (!passage.value.trim()) return;
    try { const n = await invoke("vl_add_span", { id: list.id, version: S.v1, span: passage.value.trim() }); L.flash = `Added ${n} verse${n === 1 ? "" : "s"}.`; reload(); }
    catch (e) { msg.textContent = String(e); }
  };
  passage.addEventListener("keydown", (e) => { if (e.key === "Enter") add(); });
  const sel = S.sel && S.books[S.book] ? { ns: S.v1, code: S.books[S.book].code, chapter: S.chapter, verse: S.sel.verse } : null;
  const addSel = el("button", { className: "btn small ghost", type: "button", textContent: sel ? `Add ${label(S.book, S.chapter, sel.verse)}` : "Add selected verse", disabled: !sel, title: sel ? "" : "Click a verse number in the text first" });
  addSel.addEventListener("click", async () => { try { await invoke("vl_add_refs", { id: list.id, refs: [sel] }); L.flash = "Added."; reload(); } catch (e) { msg.textContent = String(e); } });
  body.append(el("div", { className: "toolbar" }, passage, el("button", { className: "btn small", type: "button", textContent: "Add", onclick: add }), addSel));

  // list actions
  const rename = el("button", { className: "btn small ghost", type: "button", textContent: "Rename" });
  rename.addEventListener("click", () => slot.replaceChildren(inlineForm({
    placeholder: "List name", label: "Save", value: list.name, onCancel: () => slot.replaceChildren(),
    onSubmit: async (name) => { await invoke("vl_rename", { id: list.id, name, description: list.description }); reload(); },
  })));
  const sort = el("button", { className: "btn small ghost", type: "button", textContent: "Sort", title: "Canonical order, duplicates removed" });
  sort.addEventListener("click", async () => { await invoke("vl_sort", { id: list.id }); reload(); });
  const exportBtn = el("button", { className: "btn small ghost", type: "button", textContent: "Export" });
  exportBtn.addEventListener("click", async () => { const [name, text] = await invoke("vl_export", { id: list.id }); download(`${safeName(name)}.vls`, text); });
  const del = el("button", { className: "btn small ghost danger", type: "button", textContent: "Delete" });
  del.addEventListener("click", () => {
    del.textContent = "Delete this list?";
    del.classList.add("armed");
    del.onclick = async () => { await invoke("vl_delete", { id: list.id }); L.vlist = ""; reload(); };
    setTimeout(() => { if (del.isConnected) { del.textContent = "Delete"; del.classList.remove("armed"); del.onclick = null; } }, 4000);
  });
  body.append(el("div", { className: "toolbar" }, rename, sort, exportBtn, del));

  // entries
  const rows = el("div", { className: "refs" });
  body.append(el("p", { className: "sum", textContent: `${list.refs.length} verse${list.refs.length === 1 ? "" : "s"}${list.refs.length > 300 ? " (text shown for the first 300)" : ""}` }), rows);
  if (!list.refs.length) rows.append(note("This list is empty. Add a passage above."));
  list.refs.forEach((r, i) => {
    const go = el("button", { className: "ref-go", type: "button" }, el("b", { textContent: r.label }), r.ns !== S.v1 ? el("small", { className: "tag", textContent: r.ns.toUpperCase() }) : "");
    go.addEventListener("click", () => goToCode(r.code, r.chapter, r.verse, r.ns));
    const text = el("span", { className: "ref-text muted" });
    const rm = el("button", { className: "icon-btn small", type: "button", ariaLabel: `Remove ${r.label}`, title: "Remove", textContent: "×" });
    rm.addEventListener("click", async () => { await invoke("vl_remove", { id: list.id, index: i }); reload(); });
    rows.append(el("div", { className: "ref-row" }, el("div", { className: "ref-main" }, go, text), rm));
  });
  invoke("vl_texts", { id: list.id, version: S.v1 }).then((texts) => {
    [...rows.querySelectorAll(".ref-text")].forEach((n, i) => { n.textContent = texts[i] ?? ""; });
  }).catch(() => {});

  // compare with another list
  if (all.length > 1) {
    const other = el("select", { className: "field", ariaLabel: "Other list" }, ...all.filter((l) => l.id !== list.id).map((l) => el("option", { value: l.id, textContent: l.name })));
    const op = el("select", { className: "field", ariaLabel: "How to compare" }, ...OPS.map(([v, t]) => el("option", { value: v, textContent: t })));
    const name = el("input", { type: "text", className: "field", placeholder: "Name for the result", ariaLabel: "Name for the result" });
    const make = el("button", { className: "btn small", type: "button", textContent: "Create list" });
    make.addEventListener("click", async () => {
      try { L.vlist = await invoke("vl_combine", { a: list.id, b: other.value, op: op.value, name: name.value.trim() || `${list.name} · ${op.selectedOptions[0].textContent}` }); reload(); }
      catch (e) { msg.textContent = String(e); }
    });
    body.append(el("details", { className: "compare" }, el("summary", { textContent: "Compare with another list" }), el("div", { className: "stack" }, other, op, name, make)));
  }
}

/** Search results -> a new verse list (called from the Search tab). */
function saveHitsAsList(version, hits, query, mount) {
  mount.replaceChildren(inlineForm({
    placeholder: "Name for the list", label: "Save list", value: query, onCancel: () => mount.replaceChildren(),
    onSubmit: async (name) => {
      try {
        L.vlist = await invoke("vl_create", { name, refs: hits.map((h) => ({ ns: version, code: h.code, chapter: h.chapter, verse: h.verse })) });
        L.mode = "verses";
        L.flash = `Saved ${hits.length} verses.`;
        setPanel(true);
        selectTab("lists");
      } catch (e) { mount.replaceChildren(note(String(e))); }
    },
  }));
}

/* ── word lists ── */
const POS = {
  latin: [],
  greek: [["", "All words"], ["n", "Nouns"], ["v", "Verbs"], ["a", "Adjectives"], ["d", "Articles"], ["r", "Pronouns"], ["p", "Prepositions"], ["c", "Conjunctions"], ["b", "Adverbs"], ["x", "Particles"]],
  hebrew: [["", "All words"], ["n", "Nouns"], ["v", "Verbs"], ["a", "Adjectives"], ["p", "Pronouns"], ["P", "Particles"], ["u", "Numerals"]],
};

async function renderWordLists(body) {
  let saved;
  try { saved = await invoke("wl_all"); } catch (e) { body.replaceChildren(fail(e)); return; }

  // generate
  const ver = el("select", { className: "field", ariaLabel: "Version for the word list" }, ...S.versions.map((v) => el("option", { value: v.id, textContent: v.id.toUpperCase() })));
  ver.value = S.versions.some((v) => v.id === S.v1) ? S.v1 : S.versions[0].id;
  const range = el("input", { type: "text", className: "field", placeholder: "Passage (blank = whole text), e.g. Jam 3:13 - Jam 5:20", ariaLabel: "Passage" });
  const pos = el("select", { className: "field", ariaLabel: "Part of speech" });
  const fillPos = () => {
    const opts = POS[scriptOf(ver.value)];
    pos.replaceChildren(...opts.map(([v, t]) => el("option", { value: v, textContent: t })));
    pos.hidden = !opts.length;
  };
  ver.addEventListener("change", fillPos);
  fillPos();
  const status = el("p", { className: "muted small pad", role: "status" });
  const go = el("button", { className: "btn small", type: "button", textContent: "Generate" });
  go.addEventListener("click", async () => {
    status.textContent = "Counting…";
    go.disabled = true;
    try {
      const r = await invoke("wl_generate", { version: ver.value, range: range.value, pos: pos.value || null });
      L.words = { ...r, version: ver.value, id: null, name: "" };
      L.wshown = 300;
      status.textContent = "";
      drawWords();
    } catch (e) { status.textContent = String(e); }
    go.disabled = false;
  });
  range.addEventListener("keydown", (e) => { if (e.key === "Enter") go.click(); });
  body.replaceChildren(el("details", { className: "compare", open: !L.words }, el("summary", { textContent: "Make a word list" }), el("div", { className: "stack" }, ver, range, pos, go)), status);

  // saved lists
  if (saved.length) {
    const pick = el("select", { className: "field", ariaLabel: "Saved word lists" }, el("option", { value: "", textContent: "Open a saved list…" }), ...saved.map((l) => el("option", { value: l.id, textContent: `${l.name} (${l.count})` })));
    pick.value = L.wlist;
    pick.addEventListener("change", async () => {
      L.wlist = pick.value;
      if (!pick.value) return;
      const l = await invoke("wl_get", { id: pick.value });
      if (l) { L.words = { source: l.source, entries: l.entries, version: null, id: l.id, name: l.name }; L.wshown = 300; drawWords(); }
    });
    body.append(el("div", { className: "toolbar" }, pick));
    if (saved.length > 1) {
      const a = el("select", { className: "field", ariaLabel: "First list" }, ...saved.map((l) => el("option", { value: l.id, textContent: l.name })));
      const b = el("select", { className: "field", ariaLabel: "Second list" }, ...saved.map((l) => el("option", { value: l.id, textContent: l.name })));
      b.selectedIndex = 1;
      const op = el("select", { className: "field", ariaLabel: "How to compare" }, ...WOPS.map(([v, t]) => el("option", { value: v, textContent: t })));
      const cmp = el("button", { className: "btn small", type: "button", textContent: "Compare" });
      cmp.addEventListener("click", async () => {
        try {
          const entries = await invoke("wl_combine", { a: a.value, b: b.value, op: op.value });
          L.words = { source: `${a.selectedOptions[0].textContent} · ${op.selectedOptions[0].textContent.toLowerCase()} · ${b.selectedOptions[0].textContent}`, entries, version: null, id: null, name: "" };
          L.wshown = 300;
          drawWords();
        } catch (e) { status.textContent = String(e); }
      });
      body.append(el("details", { className: "compare" }, el("summary", { textContent: "Compare two saved lists" }), el("div", { className: "stack" }, a, b, op, cmp)));
    }
  }
  body.append(el("div", { className: "words", id: "words" }));
  drawWords();
}

function drawWords() {
  const box = $("#words");
  if (!box) return;
  const w = L.words;
  if (!w) { box.replaceChildren(el("div", { className: "panel-empty" }, el("strong", { textContent: "No word list yet" }), "Make one from a version and passage, or open a saved list.")); return; }
  const sc = w.version ? scriptOf(w.version) : "latin";
  const q = L.wfilter.trim().toLowerCase();
  let rows = w.entries.filter((e) => !q || e.word.toLowerCase().includes(q));
  const locale = sc === "greek" ? "el" : sc === "hebrew" ? "he" : "en";
  if (L.wsort === "alpha") rows = [...rows].sort((a, b) => a.word.localeCompare(b.word, locale));
  const total = w.entries.reduce((s, e) => s + e.count, 0);
  const max = rows.reduce((m, e) => Math.max(m, e.count), 1);

  const filter = el("input", { type: "search", className: "field", placeholder: "Filter words", value: L.wfilter, ariaLabel: "Filter words" });
  filter.addEventListener("input", () => { L.wfilter = filter.value; L.wshown = 300; drawWords(); filter.focus(); filter.setSelectionRange(filter.value.length, filter.value.length); });
  const sort = segmented([["count", "Frequency"], ["alpha", "A–Z"]], L.wsort, (m) => { L.wsort = m; drawWords(); }, "Sort");
  const save = el("button", { className: "btn small", type: "button", textContent: w.id ? "Saved" : "Save list", disabled: !!w.id });
  const slot = el("div");
  save.addEventListener("click", () => slot.replaceChildren(inlineForm({
    placeholder: "Name for the list", label: "Save", value: w.source, onCancel: () => slot.replaceChildren(),
    onSubmit: async (name) => { L.wlist = await invoke("wl_save", { name, source: w.source, entries: w.entries }); L.words = null; renderLists(); },
  })));
  const exp = el("button", { className: "btn small ghost", type: "button", textContent: "Export CSV" });
  exp.addEventListener("click", () => download(`${safeName(w.name || w.source)}.csv`, "﻿word,count\r\n" + w.entries.map((e) => `"${e.word.replace(/"/g, '""')}",${e.count}`).join("\r\n") + "\r\n", "text/csv"));
  const del = w.id ? el("button", { className: "btn small ghost danger", type: "button", textContent: "Delete" }) : null;
  if (del) del.addEventListener("click", async () => { await invoke("wl_delete", { id: w.id }); L.words = null; L.wlist = ""; renderLists(); });

  const list = el("div", { className: "wordrows", role: "list" });
  rows.slice(0, L.wshown).forEach((e) => {
    const word = el(sc === "latin" ? "button" : "span", { className: `word ${sc}`, textContent: e.word, lang: LANG[sc], dir: sc === "hebrew" ? "rtl" : "ltr" });
    if (sc === "latin") { word.type = "button"; word.title = "Search for this word"; word.addEventListener("click", () => runSearch(e.word)); }
    list.append(el("div", { className: "wordrow", role: "listitem" }, word, el("span", { className: "freq" }, el("i", { style: `width:${Math.max(2, (100 * e.count) / max)}%` })), el("span", { className: "count", textContent: e.count })));
  });
  if (rows.length > L.wshown) {
    list.append(el("button", { className: "btn small ghost", type: "button", textContent: `Show ${Math.min(300, rows.length - L.wshown)} more`, onclick: () => { L.wshown += 300; drawWords(); } }));
  }
  box.replaceChildren(
    el("p", { className: "sum", textContent: `${w.entries.length} words · ${total} occurrences · ${w.source}` }),
    el("div", { className: "toolbar" }, sort, save, exp, ...(del ? [del] : [])),
    slot,
    filter,
    rows.length ? list : note("No words match."),
  );
}
