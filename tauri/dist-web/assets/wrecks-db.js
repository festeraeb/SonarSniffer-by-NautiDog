/**
 * Great Lakes wreck database browser — reads public wrecks API.
 */
import { pickApiBase } from "./wrecks-api-base.js";

let apiBase = "";

function el(id) {
  return document.getElementById(id);
}

function fmtCoord(v) {
  if (v == null || v === "") return "—";
  return Number(v).toFixed(4);
}

function pinBadge(pc, cq) {
  if (pc) {
    const c =
      pc === "verified" ? "ok" : pc === "estimated" ? "warn" : pc === "parsed" ? "muted" : "";
    return `<span class="badge ${c}">${pc}</span>`;
  }
  return qualityBadge(cq);
}

function qualityBadge(q) {
  if (!q) return "";
  const c =
    q === "survey_verified" || q === "preserve_registry"
      ? "ok"
      : q === "swayze_parsed" || q === "place_estimated" || q === "agent_estimated"
        ? "warn"
        : "muted";
  return `<span class="badge ${c}">${q}</span>`;
}

async function loadStats() {
  const r = await fetch(`${apiBase}/stats`);
  if (!r.ok) throw new Error(`stats ${r.status}`);
  const s = await r.json();
  el("wreck-stats").innerHTML = `
    <div class="stat-grid">
      <div class="stat"><span class="stat-n">${s.total_wrecks?.toLocaleString() ?? "—"}</span><span class="stat-l">Total wrecks</span></div>
      <div class="stat"><span class="stat-n">${s.with_coordinates?.toLocaleString() ?? "—"}</span><span class="stat-l">With coordinates</span></div>
      <div class="stat"><span class="stat-n">${s.with_namag_features ?? "—"}</span><span class="stat-l">NAMAG magnetic</span></div>
      <div class="stat"><span class="stat-n">${s.steel_freighters?.toLocaleString() ?? "—"}</span><span class="stat-l">Steel freighters</span></div>
      <div class="stat"><span class="stat-n">${s.pins_by_class?.verified ?? "—"}</span><span class="stat-l">Verified pins</span></div>
      <div class="stat"><span class="stat-n">${s.pins_by_class?.estimated ?? "—"}</span><span class="stat-l">Estimated pins</span></div>
      <div class="stat"><span class="stat-n">${s.canonical_sites?.toLocaleString() ?? "—"}</span><span class="stat-l">Canonical sites</span></div>
    </div>
    <p class="forge-muted">Use the map below to toggle verified vs estimated loss sites. Swayze census is historical — not every pin is a surveyed wreck.</p>
  `;
}

async function searchWrecks(q, page = 1) {
  const limit = 25;
  let url;
  if (q && q.trim()) {
    url = `${apiBase}/wrecks/search/query?q=${encodeURIComponent(q.trim())}&limit=${limit}`;
  } else {
    url = `${apiBase}/wrecks?page=${page}&limit=${limit}`;
  }
  const r = await fetch(url);
  if (!r.ok) throw new Error(`search ${r.status}`);
  return r.json();
}

function renderResults(data, q) {
  const rows = data.results || [];
  const tbody = el("wreck-results");
  if (!rows.length) {
    tbody.innerHTML = `<tr><td colspan="6">No matches.</td></tr>`;
    return;
  }
  tbody.innerHTML = rows
    .map(
      (w) => `
    <tr>
      <td><a href="${apiBase}/wrecks/${w.id}" target="_blank" rel="noopener">${w.id}</a></td>
      <td>${escapeHtml(w.name || "")}</td>
      <td>${fmtCoord(w.latitude)}, ${fmtCoord(w.longitude)}</td>
      <td>${pinBadge(w.pin_class, w.coord_quality)} ${qualityBadge(w.coord_quality && !w.pin_class ? w.coord_quality : "")}</td>
      <td>${escapeHtml(w.hull_material || w.feature_type || "—")}</td>
      <td>${escapeHtml((w.source || "").slice(0, 24))}</td>
    </tr>`
    )
    .join("");
  const meta = q
    ? `${data.count ?? rows.length} matches for “${escapeHtml(q)}”`
    : `Page ${data.page ?? 1} of ${data.pages ?? "?"} (${data.total?.toLocaleString() ?? "?"} total)`;
  el("wreck-meta").textContent = meta;
}

function escapeHtml(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

let currentPage = 1;
let lastQuery = "";

async function runSearch(page = 1) {
  const q = el("wreck-search").value.trim();
  lastQuery = q;
  currentPage = page;
  el("wreck-banner").className = "forge-banner pending";
  el("wreck-banner").textContent = "Loading…";
  try {
    const data = await searchWrecks(q, page);
    renderResults(data, q);
    el("wreck-banner").className = "forge-banner ok";
    el("wreck-banner").textContent = `API: ${apiBase}`;
  } catch (e) {
    el("wreck-banner").className = "forge-banner err";
    el("wreck-banner").textContent = `Failed: ${e.message}`;
  }
}

export async function initWrecksDb() {
  el("wreck-banner").textContent = "Connecting to API…";
  const base = await pickApiBase();
  apiBase = base || "";
  if (!base) {
    el("wreck-banner").className = "forge-banner err";
    el("wreck-banner").textContent = "Wrecks API unreachable.";
    return;
  }
  el("api-link").href = `${apiBase}/docs`;
  el("kml-link").href = `${apiBase}/wrecks/live.kml`;
  try {
    await loadStats();
    await runSearch(1);
  } catch (e) {
    el("wreck-banner").className = "forge-banner err";
    el("wreck-banner").textContent = e.message;
  }
  el("wreck-search-btn").addEventListener("click", () => runSearch(1));
  el("wreck-search").addEventListener("keydown", (ev) => {
    if (ev.key === "Enter") runSearch(1);
  });
  el("wreck-prev").addEventListener("click", () => {
    if (!lastQuery && currentPage > 1) runSearch(currentPage - 1);
  });
  el("wreck-next").addEventListener("click", () => {
    if (!lastQuery) runSearch(currentPage + 1);
  });
}

initWrecksDb();
