"use strict";
/* Diagrams tab of the study tools: Reed-Kellogg sentence diagrams of the Greek New Testament (Randy Leedy's,
   shipped with BibleWorks) and the grammar examples. The backend draws each as an SVG.
   Loaded after maps.js; shares STUDIO, el, invoke, store, note, fail, fieldRow, goToCode, S. */

const DG = { list: null, book: store.get("dgBook", ""), open: null, zoom: 1, req: 0 };

async function renderDiagrams(body) {
  body.className = "st-body st-dgm";
  body.append(note("Loading diagrams…"));
  try { DG.list ??= await invoke("dg_list"); } catch (e) { return body.replaceChildren(el("div", { className: "empty" }, el("h2", { textContent: "No diagrams found" }), el("p", { textContent: String(e) }))); }
  drawDiagrams(body);
}

function dgLabel(d) {
  const bn = S.books.find((b) => b.code === d.book)?.name ?? d.book;
  const end = d.end_chapter !== d.chapter ? `${d.end_chapter}:${d.end_verse}` : d.end_verse !== d.verse ? `${d.end_verse}` : "";
  return `${bn} ${d.chapter}:${d.verse}${end ? "–" + end : ""}`;
}

function drawDiagrams(body) {
  body.replaceChildren();
  const code = S.books[S.book]?.code, here = DG.list.gnt.filter((d) => d.book === code && S.chapter >= d.chapter && S.chapter <= d.end_chapter);
  const books = [...new Set(DG.list.gnt.map((d) => d.book))];
  if (!DG.book) DG.book = here[0]?.book ?? books[0];
  const listBox = el("div", { className: "dg-list", role: "listbox", ariaLabel: "Diagrams" });
  const bookSel = el("select", { className: "field", ariaLabel: "Book" }, ...books.map((b) => el("option", { value: b, textContent: S.books.find((x) => x.code === b)?.name ?? b, selected: b === DG.book })));
  bookSel.addEventListener("change", () => { DG.book = bookSel.value; store.set("dgBook", DG.book); drawList(); });

  const item = (label, name, example, sub) => {
    const b = el("button", { type: "button", className: "dg-item", role: "option", textContent: label });
    b.setAttribute("aria-selected", String(DG.open?.name === name && DG.open.example === example));
    if (sub) b.append(el("small", { className: "muted", textContent: sub }));
    b.addEventListener("click", () => show(name, example, label));
    return b;
  };
  function drawList() {
    const rows = [];
    if (here.length) rows.push(el("h3", { className: "dg-h", textContent: "Where you are reading" }), ...here.map((d) => item(dgLabel(d), d.name, false)));
    rows.push(el("h3", { className: "dg-h", textContent: "Whole book" }), ...DG.list.gnt.filter((d) => d.book === DG.book).map((d) => item(dgLabel(d), d.name, false)));
    if (DG.list.examples.length) rows.push(el("h3", { className: "dg-h", textContent: "Grammar examples" }), ...DG.list.examples.map((n) => item(n.replace(/^example_/, ""), n, true)));
    listBox.replaceChildren(...rows);
  }

  const stage = el("div", { className: "dg-stage", tabIndex: 0, role: "region", ariaLabel: "Diagram" });
  const head = el("div", { className: "dg-head" });
  const zb = (f, t) => el("button", { className: "icon-btn", type: "button", textContent: t, title: f > 1 ? "Zoom in" : "Zoom out", ariaLabel: f > 1 ? "Zoom in" : "Zoom out", onclick: () => { DG.zoom = Math.min(4, Math.max(0.3, DG.zoom * f)); fit(); } });
  const tools = el("div", { className: "dg-tools" }, zb(1 / 1.25, "−"), zb(1.25, "+"));
  const caption = el("p", { className: "dg-text greek", lang: "grc" });
  body.append(el("div", { className: "dg-wrap" },
    el("div", { className: "dg-side" }, fieldRow("Book", bookSel), listBox),
    el("div", { className: "dg-main" }, el("div", { className: "dg-bar" }, head, tools), stage, caption)));
  drawList();
  stage.style.setProperty("--dgm-bg", "var(--raised)");

  function fit() {
    const svg = stage.querySelector("svg");
    if (!svg) return;
    const vb = svg.viewBox.baseVal;
    svg.removeAttribute("height");
    svg.setAttribute("width", String(Math.round(vb.width * DG.zoom)));
    svg.style.maxWidth = "none";
  }

  async function show(name, example, label) {
    const my = ++DG.req;
    DG.open = { name, example };
    drawList();
    try {
      const d = await invoke("dg_open", { name, example });
      if (my !== DG.req) return;
      const doc = new DOMParser().parseFromString(d.svg, "image/svg+xml").documentElement;
      stage.replaceChildren(document.importNode(doc, true));
      DG.zoom = 1;
      fit();
      stage.scrollTo(0, 0);
      head.replaceChildren(el("strong", { textContent: label }), d.description || d.title ? el("span", { className: "muted", textContent: d.description || d.title }) : "");
      caption.textContent = d.text;
      const gnt = !example && DG.list.gnt.find((x) => x.name === name);
      if (gnt) head.append(el("button", { className: "btn ghost small", type: "button", textContent: "Read this passage", onclick: () => { closeStudio(); goToCode(gnt.book, gnt.chapter, gnt.verse, "kjv"); } }));
    } catch (e) { if (my === DG.req) stage.replaceChildren(fail(e)); }
  }
  if (DG.open) show(DG.open.name, DG.open.example, DG.open.example ? DG.open.name.replace(/^example_/, "") : dgLabel(DG.list.gnt.find((d) => d.name === DG.open.name) ?? { book: "", chapter: 0, verse: 0, end_chapter: 0, end_verse: 0 }));
  else stage.append(el("div", { className: "empty" }, el("h2", { textContent: "Sentence diagrams" }), el("p", { textContent: here.length ? "Pick a diagram on the left. These are the ones for the chapter you are reading." : "Pick a book and a passage on the left, or one of the grammar examples." })));
}

STUDIO.tabs.push({ id: "diagrams", label: "Diagrams", render: renderDiagrams });
