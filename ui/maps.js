"use strict";
/* Maps tab of the study tools: BibleWorks map data drawn by the backend (base picture + SVG overlay).
   Loaded after studio.js; shares its globals (STUDIO, el, svg, invoke, store, segmented, note, fail, fieldRow). */

const MP = {
  cat: null,
  style: store.get("mapStyle", "relief"),
  vp: store.get("mapVp", { lat: 31.78, lon: 35.22, ppd: 150 }),
  overlays: store.get("mapOverlays", []),
  dark: false,
  view: "",
  marker: null,
  req: 0,
  timer: null,
};
const MAP_STYLES = [["relief", "Relief"], ["satellite", "Satellite"], ["elevation", "Elevation"], ["landcover", "Land cover"]];
const cleanLabel = (s) => s.replace(/\s+vector overlay$/i, "");
const svgIcon = (d) => svg("svg", { class: "ico", viewBox: "0 0 16 16", "aria-hidden": "true" }, svg("path", { d }));

async function renderMaps(body) {
  body.className = "st-body st-maps";
  body.append(note("Loading maps…"));
  try { MP.cat ??= await invoke("map_catalog"); } catch (e) { return body.replaceChildren(el("div", { className: "empty" }, el("h2", { textContent: "No map data" }), el("p", { textContent: String(e) }))); }
  drawMaps(body);
}

function drawMaps(body) {
  body.replaceChildren();
  const cat = MP.cat;
  const persist = () => { store.set("mapVp", MP.vp); store.set("mapOverlays", MP.overlays); };

  // saved views, grouped by their folder
  const viewSel = el("select", { className: "field", ariaLabel: "Saved view" }, el("option", { value: "", textContent: "Choose a saved view…" }));
  const groups = new Map();
  for (const v of cat.views) {
    const i = v.id.lastIndexOf("/"), g = i < 0 ? "Views" : v.id.slice(0, i);
    if (!groups.has(g)) groups.set(g, el("optgroup", { label: g }));
    groups.get(g).append(el("option", { value: v.id, textContent: v.title || v.id.slice(i + 1), selected: v.id === MP.view }));
  }
  viewSel.append(...groups.values());
  viewSel.addEventListener("change", async () => {
    if (!viewSel.value) return;
    try {
      const v = await invoke("map_view", { id: viewSel.value });
      MP.view = viewSel.value;
      MP.vp = { lat: v.lat, lon: v.lon, ppd: v.ppd };
      MP.overlays = v.overlays;
      MP.dark = v.white_on_black;
      MP.marker = null;
      persist();
      drawOverlayList();
      paint();
    } catch (e) { status.textContent = String(e); }
  });

  const styleSeg = () => segmented(MAP_STYLES, MP.style, (id) => { MP.style = id; store.set("mapStyle", id); styleBox.replaceChildren(styleSeg()); paint(); }, "Base map");
  const styleBox = el("div", { className: "map-seg" }, styleSeg());

  const q = el("input", { type: "search", className: "field", placeholder: "Jerusalem, Corinth, Sea of Galilee…", ariaLabel: "Find a place" });
  const found = el("ul", { className: "map-found" });
  let findTimer;
  q.addEventListener("input", () => {
    clearTimeout(findTimer);
    findTimer = setTimeout(async () => {
      if (!q.value.trim()) return found.replaceChildren();
      try {
        const hits = await invoke("map_find", { query: q.value });
        found.replaceChildren(...(hits.length ? hits.map((h) => el("li", {}, el("button", { type: "button", className: "map-hit", onclick: () => {
          MP.vp = { lat: h.lat, lon: h.lon, ppd: Math.max(MP.vp.ppd, 120) };
          MP.marker = { lat: h.lat, lon: h.lon };
          persist();
          paint();
        } }, el("span", { textContent: h.name }), el("small", { className: "muted", textContent: h.classification || h.overlay })))) : [el("li", { className: "muted", textContent: "No places match." })]));
      } catch (e) { found.replaceChildren(fail(e)); }
    }, 200);
  });

  const overlayBox = el("div", { className: "map-overlays" });
  const addSel = el("select", { className: "field", ariaLabel: "Add an overlay" });
  function drawOverlayList() {
    const byId = new Map(cat.overlays.map((o) => [o.id, o]));
    overlayBox.replaceChildren(...MP.overlays.map((id) => {
      const o = byId.get(id);
      return el("div", { className: "map-ov" },
        el("span", { textContent: cleanLabel(o?.label || id) }),
        el("button", { className: "icon-btn", type: "button", title: "Remove", ariaLabel: `Remove ${cleanLabel(o?.label || id)}`, onclick: () => { MP.overlays = MP.overlays.filter((x) => x !== id); persist(); drawOverlayList(); paint(); } }, svgIcon("m4 4 8 8M12 4l-8 8")));
    }));
    if (!MP.overlays.length) overlayBox.append(el("p", { className: "muted map-none", textContent: "No overlays. Add routes, sites, rivers or boundaries below." }));
    addSel.replaceChildren(el("option", { value: "", textContent: "Add an overlay…" }));
    const og = new Map();
    for (const o of cat.overlays) {
      if (MP.overlays.includes(o.id)) continue;
      const g = o.group.join(" › ") || (o.kind === "lines" ? "Lines" : "Other");
      if (!og.has(g)) og.set(g, el("optgroup", { label: g }));
      og.get(g).append(el("option", { value: o.id, textContent: cleanLabel(o.label || o.id) }));
    }
    addSel.append(...og.values());
  }
  addSel.addEventListener("change", () => { if (addSel.value) { MP.overlays = [...MP.overlays, addSel.value]; persist(); drawOverlayList(); paint(); } });

  const status = el("p", { className: "muted map-status", role: "status" });
  const side = el("div", { className: "map-side" },
    fieldRow("View", viewSel), fieldRow("Base map", styleBox), fieldRow("Find a place", q), found,
    el("h3", { className: "st-sub", textContent: "Overlays" }), overlayBox, addSel, status);

  const canvas = el("canvas", { className: "map-base", ariaHidden: "true" });
  const vec = el("div", { className: "map-vec", ariaHidden: "true" });
  const zbtn = (f) => el("button", { className: "icon-btn", type: "button", textContent: f > 1 ? "+" : "−", title: f > 1 ? "Zoom in" : "Zoom out", ariaLabel: f > 1 ? "Zoom in" : "Zoom out", onclick: () => zoomAt(null, f) });
  const stage = el("div", { className: "map-stage", tabIndex: 0, role: "application", ariaLabel: "Map. Drag to pan, scroll or use plus and minus to zoom." },
    canvas, vec, el("div", { className: "map-zoom" }, zbtn(1.5), zbtn(1 / 1.5)));
  body.append(el("div", { className: "map-wrap" }, side, stage));
  drawOverlayList();

  const size = () => ({ w: Math.max(64, Math.round(stage.clientWidth)), h: Math.max(64, Math.round(stage.clientHeight)) });

  async function paint() {
    const my = ++MP.req, { w, h } = size(), { lat, lon, ppd } = MP.vp;
    stage.classList.toggle("dark", MP.dark);
    status.textContent = "";
    try {
      const [png, svgText] = await Promise.all([
        invoke("map_render", { style: MP.style, lat, lon, ppd, width: w, height: h }),
        MP.overlays.length ? invoke("map_vectors", { overlays: MP.overlays, whiteOnBlack: MP.dark, lat, lon, ppd, width: w, height: h }) : "",
      ]);
      if (my !== MP.req) return;
      const bmp = await createImageBitmap(new Blob([png], { type: "image/png" }));
      canvas.width = w;
      canvas.height = h;
      canvas.getContext("2d").drawImage(bmp, 0, 0);
      canvas.style.transform = vec.style.transform = "";
      vec.replaceChildren();
      if (svgText) vec.append(document.importNode(new DOMParser().parseFromString(svgText, "image/svg+xml").documentElement, true));
      if (MP.marker) {
        const x = (MP.marker.lon - lon) * ppd + w / 2, y = (lat - MP.marker.lat) * ppd + h / 2;
        vec.append(svg("svg", { class: "map-pin", width: w, height: h }, svg("circle", { cx: x, cy: y, r: 9, class: "pin-ring" }), svg("circle", { cx: x, cy: y, r: 3, class: "pin-dot" })));
      }
    } catch (e) { if (my === MP.req) status.textContent = String(e); }
  }

  const later = () => { clearTimeout(MP.timer); MP.timer = setTimeout(paint, 140); };
  function zoomAt(pt, f) {
    const { w, h } = size(), { lat, lon, ppd } = MP.vp;
    const nppd = Math.min(5000, Math.max(8, ppd * f));
    const [cx, cy] = pt ?? [w / 2, h / 2];
    const plat = lat + (h / 2 - cy) / ppd, plon = lon + (cx - w / 2) / ppd; // the point under the cursor stays put
    MP.vp = { ppd: nppd, lat: plat + (cy - h / 2) / nppd, lon: plon - (cx - w / 2) / nppd };
    persist();
    later();
  }
  stage.addEventListener("wheel", (e) => { e.preventDefault(); const r = stage.getBoundingClientRect(); zoomAt([e.clientX - r.left, e.clientY - r.top], e.deltaY < 0 ? 1.25 : 1 / 1.25); }, { passive: false });
  let drag = null;
  stage.addEventListener("pointerdown", (e) => { if (e.button !== 0 || e.target.closest("button")) return; drag = { x: e.clientX, y: e.clientY }; stage.setPointerCapture(e.pointerId); stage.classList.add("grab"); });
  stage.addEventListener("pointermove", (e) => { if (drag) canvas.style.transform = vec.style.transform = `translate(${e.clientX - drag.x}px, ${e.clientY - drag.y}px)`; });
  const end = (e) => {
    if (!drag) return;
    const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
    drag = null;
    stage.classList.remove("grab");
    if (Math.abs(dx) + Math.abs(dy) < 2) return;
    MP.vp = { ...MP.vp, lat: MP.vp.lat + dy / MP.vp.ppd, lon: MP.vp.lon - dx / MP.vp.ppd };
    persist();
    paint();
  };
  stage.addEventListener("pointerup", end);
  stage.addEventListener("pointercancel", end);
  stage.addEventListener("keydown", (e) => {
    const step = 80 / MP.vp.ppd, k = e.key;
    if (k === "+" || k === "=") return void (e.preventDefault(), zoomAt(null, 1.5));
    if (k === "-") return void (e.preventDefault(), zoomAt(null, 1 / 1.5));
    const d = { ArrowLeft: [0, -step], ArrowRight: [0, step], ArrowUp: [step, 0], ArrowDown: [-step, 0] }[k];
    if (!d) return;
    e.preventDefault();
    MP.vp = { ...MP.vp, lat: MP.vp.lat + d[0], lon: MP.vp.lon + d[1] };
    persist();
    later();
  });
  const ro = new ResizeObserver(later);
  ro.observe(stage);
  STUDIO.cleanup = () => { ro.disconnect(); clearTimeout(MP.timer); };
  paint();
}

STUDIO.tabs.push({ id: "maps", label: "Maps", render: renderMaps });
