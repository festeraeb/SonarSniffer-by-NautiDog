/**
 * Corner HUD: fleet GPU temps (T440 + cesarops-node heartbeats) + named LLM tok/s.
 * Pages push lines via window.ForgeHud.log("message", "info"|"ok"|"warn"|"err").
 */
(function () {
  "use strict";

  const FORGE_CANDIDATES = [
    window.location.origin,
    "https://api.cesarops.org",
    "http://10.0.0.61:9100",
    "http://100.72.182.77:9100",
  ];

  const PEAK_TFLOPS_FP16 = [
    [/V100.*32/i, 125],
    [/V100/i, 18.7],
    [/A100/i, 312],
    [/RTX 4090/i, 330],
    [/2060\s*SUPER/i, 44],
    [/2060/i, 40],
    [/GTX 1070/i, 19.3],
    [/P106/i, 3.8],
  ];

  const POLL_MS = 4000;
  const MAX_LOG_LINES = 48;
  let apiBase = null;
  let collapsed = false;
  const logLines = [];

  function peakTflops(name) {
    if (!name) return null;
    for (const [re, t] of PEAK_TFLOPS_FP16) {
      if (re.test(name)) return t;
    }
    return null;
  }

  function effTflops(peak, utilPct) {
    if (peak == null || utilPct == null) return null;
    return (peak * (utilPct / 100)).toFixed(1);
  }

  function ts() {
    return new Date().toLocaleTimeString("en-GB", { hour12: false });
  }

  function stripHtml(s) {
    const d = document.createElement("div");
    d.innerHTML = s;
    return d.textContent || d.innerText || "";
  }

  function renderLogs() {
    const box = document.querySelector("#forge-gpu-hud .forge-hud-logs");
    if (!box) return;
    if (!logLines.length) {
      box.innerHTML = '<div class="forge-hud-log-line">No log lines yet</div>';
      return;
    }
    box.innerHTML = logLines
      .map(
        (l) =>
          `<div class="forge-hud-log-line ${l.cls}">` +
          `<span class="ts">[${l.ts}]</span> ${escapeHtml(l.msg)}</div>`
      )
      .join("");
    box.scrollTop = box.scrollHeight;
  }

  function escapeHtml(s) {
    return String(s)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
  }

  async function fetchJson(url, ms) {
    const ctrl = new AbortController();
    const t = setTimeout(() => ctrl.abort(), ms);
    try {
      const res = await fetch(url, { cache: "no-store", signal: ctrl.signal });
      if (!res.ok) throw new Error(String(res.status));
      return await res.json();
    } finally {
      clearTimeout(t);
    }
  }

  async function isForgeBase(base) {
    try {
      const j = await fetchJson(`${base.replace(/\/$/, "")}/health`, 6000);
      return j && j.service === "cesarops-forge-v2";
    } catch {
      return false;
    }
  }

  async function resolveBase() {
    const saved = localStorage.getItem("forge_api_base");
    if (saved && (await isForgeBase(saved))) return saved.replace(/\/$/, "");

    const seen = new Set();
    for (const raw of FORGE_CANDIDATES) {
      if (!raw || seen.has(raw)) continue;
      seen.add(raw);
      const base = raw.replace(/\/$/, "");
      if (await isForgeBase(base)) {
        localStorage.setItem("forge_api_base", base);
        return base;
      }
    }
    return null;
  }

  function ensureHud() {
    if (document.getElementById("forge-gpu-hud")) return document.getElementById("forge-gpu-hud");

    const el = document.createElement("aside");
    el.id = "forge-gpu-hud";
    el.setAttribute("role", "status");
    el.setAttribute("aria-live", "polite");
    el.innerHTML =
      '<div class="forge-hud-head" title="Click to expand/collapse">' +
      '<span class="forge-hud-title">Forge</span>' +
      '<span class="forge-hud-summary">…</span>' +
      "</div>" +
      '<div class="forge-hud-body"></div>' +
      '<div class="forge-hud-llm">LLM …</div>' +
      '<div class="forge-hud-logs-wrap">' +
      '<span class="forge-hud-logs-label">Logs</span>' +
      '<div class="forge-hud-logs"></div>' +
      "</div>";

    el.querySelector(".forge-hud-head").addEventListener("click", () => {
      collapsed = !collapsed;
      el.classList.toggle("collapsed", collapsed);
    });

    document.body.appendChild(el);
    renderLogs();
    return el;
  }

  function formatRole(slot) {
    if (!slot) return null;
    const label = slot.label || slot.role || "?";
    if (!slot.online) return `${label} off`;
    const tps = slot.tps != null ? `${Number(slot.tps).toFixed(1)} tok/s` : "ping";
    return `${label} ${tps}`;
  }

  function renderGpuRow(g) {
    const util = g.utilization_pct ?? 0;
    const temp = g.temperature_c ?? g.temp_c;
    const peak = peakTflops(g.name);
    const eff = effTflops(peak, util);
    const hot = util > 5 || (temp != null && temp >= 70);
    const host = g.host || g.node || "local";
    const label = (g.name || "GPU").replace(/NVIDIA\s+/i, "").replace(/Tesla\s+/i, "").slice(0, 14);
    const idx = g.index ?? g.id ?? "?";
    const tempStr = temp != null ? `${temp}°C · ` : "";
    return (
      `<div class="forge-hud-row${hot ? " hot" : ""}">` +
      `<span title="${escapeHtml(host)}">${escapeHtml(host)} ${label}#${idx}</span>` +
      `<b>${tempStr}${util}% · ${eff != null ? eff + " TFLOPS" : "idle"}</b>` +
      `</div>`
    );
  }

  function render(hud, mon, ping, offline) {
    const body = hud.querySelector(".forge-hud-body");
    const summary = hud.querySelector(".forge-hud-summary");
    const llm = hud.querySelector(".forge-hud-llm");

    hud.classList.toggle("offline", !!offline);

    if (offline || !mon || !mon.gpus || !mon.gpus.length) {
      summary.textContent = offline ? "offline" : "no GPUs";
      body.innerHTML = offline
        ? '<div class="forge-hud-row"><span>API</span><b>unreachable</b></div>'
        : "";
      llm.textContent = offline ? "Forge unreachable" : "Waiting for metrics…";
      return;
    }

    let maxUtil = 0;
    let maxTemp = 0;
    let sumEff = 0;
    let activeGpus = 0;

    body.innerHTML = mon.gpus.map((g) => {
      const util = g.utilization_pct ?? 0;
      const temp = g.temperature_c ?? 0;
      if (util > 5) activeGpus += 1;
      if (util > maxUtil) maxUtil = util;
      if (temp > maxTemp) maxTemp = temp;
      const peak = peakTflops(g.name);
      const eff = effTflops(peak, util);
      if (eff != null) sumEff += parseFloat(eff);
      return renderGpuRow(g);
    }).join("");

    summary.textContent =
      activeGpus > 0
        ? `${maxTemp}°C · ${maxUtil}% · ~${sumEff.toFixed(1)} TFLOPS`
        : maxTemp > 0
          ? `${maxTemp}°C · idle`
          : `${maxUtil}% idle`;

    if (ping) {
      const roles = ping.roles || {};
      const parts = [];
      const order = ["coder", "reviewer", "thinker", "draft"];
      for (const key of order) {
        const line = formatRole(roles[key] || (key === "coder" ? ping.main_engine : key === "reviewer" ? ping.p1000_validator : null));
        if (line) parts.push(line);
      }
      const benchNote = ping.bench_ran === false ? " (ping)" : "";
      llm.textContent = parts.length ? parts.join(" · ") + benchNote : "LLM ping pending…";
      llm.title = JSON.stringify(roles, null, 0);
    } else {
      llm.textContent = "LLM ping pending…";
    }
  }

  async function tick() {
    const hud = ensureHud();
    if (!apiBase) apiBase = await resolveBase();
    if (!apiBase) {
      render(hud, null, null, true);
      return;
    }

    try {
      const [mon, ping] = await Promise.all([
        fetchJson(`${apiBase}/monitor`, 10000),
        fetchJson(`${apiBase}/validate/ping`, 120000).catch(() => null),
      ]);
      render(hud, mon, ping, false);
    } catch {
      apiBase = null;
      render(hud, null, null, true);
    }
  }

  window.ForgeHud = {
    log(message, cls) {
      ensureHud();
      const kind = (cls || "info").replace(/^log-/, "");
      logLines.push({ ts: ts(), msg: stripHtml(message), cls: kind });
      if (logLines.length > MAX_LOG_LINES) logLines.shift();
      renderLogs();
    },
    clear() {
      logLines.length = 0;
      renderLogs();
    },
  };

  function start() {
    ensureHud();
    window.ForgeHud.log("Forge HUD ready (dynamic roles + fleet temps)", "ok");
    tick();
    setInterval(tick, POLL_MS);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", start);
  } else {
    start();
  }
})();
