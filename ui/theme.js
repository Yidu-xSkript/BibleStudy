"use strict";
// Loaded in <head> so the saved theme applies before first paint. No saved choice = follow the system.
(function () {
  const KEY = "bs.theme", root = document.documentElement, mq = matchMedia("(prefers-color-scheme: dark)");
  try {
    const saved = localStorage.getItem(KEY);
    if (saved === "light" || saved === "dark") root.dataset.theme = saved;
  } catch { /* storage unavailable: follow the system */ }

  const effective = () => root.dataset.theme || (mq.matches ? "dark" : "light");
  const svg = (d) => `<svg class="ico" viewBox="0 0 16 16" aria-hidden="true">${d}</svg>`;
  const SUN = svg('<circle cx="8" cy="8" r="2.8"/><path d="M8 1.5v1.4M8 13.1v1.4M1.5 8h1.4M13.1 8h1.4M3.4 3.4l1 1M11.6 11.6l1 1M3.4 12.6l1-1M11.6 4.4l1-1"/>');
  const MOON = svg('<path d="M13.2 9.6A5.6 5.6 0 0 1 6.4 2.8a5.6 5.6 0 1 0 6.8 6.8z"/>');

  function paint() {
    const b = document.getElementById("theme-btn");
    if (!b) return;
    const dark = effective() === "dark";
    b.setAttribute("aria-pressed", String(dark));
    b.innerHTML = dark ? SUN : MOON; // static icon markup
  }

  document.addEventListener("DOMContentLoaded", () => {
    paint();
    document.getElementById("theme-btn").addEventListener("click", () => {
      root.dataset.theme = effective() === "dark" ? "light" : "dark";
      try { localStorage.setItem(KEY, root.dataset.theme); } catch { /* not persisted */ }
      paint();
    });
  });
  mq.addEventListener("change", paint);
})();
