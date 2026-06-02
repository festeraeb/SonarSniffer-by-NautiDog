/**
 * Corner HUD: live fleet GPUs (NVML / nvtop-class via GET /gpu/stream SSE) + LLM tok/s.
 * Falls back to GET /monitor poll if EventSource unavailable.
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
  const HISTORY_LEN = 24;
  const MAX_LOG_LINES = 48;
  let apiBase = null;
  let collapsed = false;
  let sse = null;
  const logLines = [];
  const utilHistory = new Map();
  let cachedFleet = [];

  function cfgGpuKey(g) {
    const host = String(g.host || g.node || "local");
    const idx = g.cuda_index ?? g.index ?? g.id ?? 0;
    return `${host}:${idx}`;
  }

  function enrichMonitorWithFleet(mon) {
    if (!mon || !Array.isArray(mon.gpus)) return mon;
    if (!cachedFleet.length) return mon;

    const out = { ...mon, gpus: [...mon.gpus] };
    const seen = new Set(out.gpus.map(cfgGpuKey));

    for (const g of cachedFleet) {
      if (!g || !g.remote) continue;
      const key = cfgGpuKey(g);
      if (seen.has(key)) continue;
      seen.add(key);
      out.gpus.push({
        index: g.cuda_index ?? g.id ?? 0,
        name: g.name || "GPU",
        temperature_c: 0,
        utilization_pct: 0,
        memory_used_mb: 0,
        memory_total_mb: Number(g.vram_mb || 0),
        power_draw_w: 0,
        host: g.host || g.node || "remote",
        node: g.host || g.node || "remote",
      });
    }
    return out;
  }

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

  function escapeHtml(s) {
    return String(s)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
  }

  function gpuKey(g) {
    return `${g.host || g.node || "local"}:${g.index ?? g.id ?? 0}`;
  }

  function pushHistory(key, util) {
    if (!utilHistory.has(key)) utilHistory.set(key, []);
    const h = utilHistory.get(key);
    h.push(util);
    if (h.length > HISTORY_LEN) h.shift();
  }

  function sparkline(values) {
    if (!values.length) return "";
    const blocks = "▁▂▃▄▅▆▇█";
    const max = Math.max(...values, 1);
    return values
      .map((v) => blocks[Math.min(7, Math.round((v / max) * 7))])
      .join("");
  }

  function bar(pct, cls) {
    const p = Math.max(0, Math.min(100, Number(pct) || 0));
    return (
      `<span class="forge-hud-bar ${cls}">` +
      `<span class="fill" style="width:${p}%"></span></span>`
    );
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
    const existing = document.getElementById("forge-gpu-hud");
    if (existing) {
      placeHud(existing);
      return existing;
    }

    const el = document.createElement("aside");
    el.id = "forge-gpu-hud";
    el.setAttribute("role", "status");
    el.setAttribute("aria-live", "polite");
    el.innerHTML =
      '<div class="forge-hud-head" title="Click to expand/collapse">' +
      '<span class="forge-hud-title">Forge · live GPU</span>' +
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

    placeHud(el);
    renderLogs();
    return el;
  }

  function placeHud(el) {
    const sidebar = document.querySelector(".sidebar");
    if (sidebar) {
      if (el.parentElement !== sidebar) sidebar.appendChild(el);
      el.classList.add("embedded");
      el.classList.remove("inline");
    } else {
      let anchor = document.getElementById("forge-hud-anchor");
      if (!anchor) {
        anchor = document.createElement("div");
        anchor.id = "forge-hud-anchor";
        const host = document.querySelector("header") || document.body.firstElementChild;
        if (host && host.parentElement === document.body) host.insertAdjacentElement("afterend", anchor);
        else document.body.prepend(anchor);
      }
      if (el.parentElement !== anchor) anchor.appendChild(el);
      el.classList.remove("embedded");
      el.classList.add("inline");
    }
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
    const memUtil = g.memory_util_pct ?? Math.round(((g.memory_used_mb ?? 0) / Math.max(g.memory_total_mb ?? 1, 1)) * 100);
    const temp = g.temperature_c ?? g.temp_c;
    const peak = peakTflops(g.name);
    const eff = effTflops(peak, util);
    const hot = util > 5 || (temp != null && temp >= 70);
    const host = g.host || g.node || "local";
    const label = (g.name || "GPU").replace(/NVIDIA\s+/i, "").replace(/Tesla\s+/i, "").slice(0, 16);
    const idx = g.index ?? g.id ?? "?";
    const key = gpuKey(g);
    pushHistory(key, util);
    const spark = sparkline(utilHistory.get(key) || []);
    const tempStr = temp != null ? `${temp}°C` : "";
    const power = g.power_draw_w != null && g.power_draw_w > 0 ? `${Number(g.power_draw_w).toFixed(0)}W` : "";
    const procs = Array.isArray(g.processes) ? g.processes : [];
    const procLine =
      procs.length > 0
        ? procs
            .slice(0, 2)
            .map((p) => `${p.name || "?"} ${p.sm_pct ?? 0}%`)
            .join(" · ")
        : "";

    return (
      `<div class="forge-hud-gpu${hot ? " hot" : ""}">` +
      `<div class="forge-hud-row">` +
      `<span title="${escapeHtml(host)}">${escapeHtml(host)} ${escapeHtml(label)}#${idx}</span>` +
      `<b>${tempStr}${tempStr && power ? " · " : ""}${power} · ${util}%</b>` +
      `</div>` +
      `<div class="forge-hud-bars">` +
      `<span class="lbl">SM</span>${bar(util, "sm")}` +
      `<span class="lbl">MEM</span>${bar(memUtil, "mem")}` +
      `<span class="spark" title="util history">${spark}</span>` +
      `</div>` +
      (procLine ? `<div class="forge-hud-proc">${escapeHtml(procLine)}</div>` : "") +
      (eff != null ? `<div class="forge-hud-eff">~${eff} TFLOPS eff</div>` : "") +
      `</div>`
    );
  }

  function render(hud, mon, ping, offline, live) {
    const body = hud.querySelector(".forge-hud-body");
    const summary = hud.querySelector(".forge-hud-summary");
    const llm = hud.querySelector(".forge-hud-llm");

    hud.classList.toggle("offline", !!offline);
    hud.classList.toggle("live", !!live);

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

    const liveTag = live ? "● " : "";
    summary.textContent =
      activeGpus > 0
        ? `${liveTag}${maxTemp}°C · ${maxUtil}% · ~${sumEff.toFixed(1)} TFLOPS`
        : maxTemp > 0
          ? `${liveTag}${maxTemp}°C · idle`
          : `${liveTag}${maxUtil}% idle`;

    if (ping) {
      const roles = ping.roles || {};
      const parts = [];
      const order = ["coder", "reviewer", "thinker", "draft"];
      for (const key of order) {
        const line = formatRole(
          roles[key] ||
            (key === "coder"
              ? ping.main_engine
              : key === "reviewer"
                ? ping.p1000_validator
                : null)
        );
        if (line) parts.push(line);
      }
      const benchNote = ping.bench_ran === false ? " (ping)" : "";
      llm.textContent = parts.length ? parts.join(" · ") + benchNote : "LLM ping pending…";
    } else if (!llm.dataset.pingPending) {
      llm.textContent = "LLM ping (background)…";
    }
  }

  async function fetchPing() {
    if (!apiBase) return null;
    return fetchJson(`${apiBase}/validate/ping`, 120000).catch(() => null);
  }

  async function refreshFleetCache() {
    if (!apiBase) return;
    try {
      const d = await fetchJson(`${apiBase}/cluster/gpus`, 12000);
      cachedFleet = Array.isArray(d?.gpus) ? d.gpus : [];
    } catch {
      /* keep last cache */
    }
  }

  let lastPing = null;
  let pingTimer = null;

  function applyMonitor(mon, fromStream) {
    const hud = ensureHud();
    render(hud, enrichMonitorWithFleet(mon), lastPing, false, fromStream);
  }

  async function pollOnce() {
    if (!apiBase) apiBase = await resolveBase();
    const hud = ensureHud();
    if (!apiBase) {
      render(hud, null, null, true, false);
      return;
    }
    try {
      await refreshFleetCache();
      const mon = await fetchJson(`${apiBase}/monitor`, 10000);
      applyMonitor(mon, false);
    } catch {
      apiBase = null;
      render(hud, null, null, true, false);
    }
  }

  function connectGpuStream() {
    if (!apiBase || typeof EventSource === "undefined") return;
    if (sse) {
      sse.close();
      sse = null;
    }
    const url = `${apiBase}/gpu/stream`;
    try {
      sse = new EventSource(url);
      sse.onmessage = (ev) => {
        try {
          const data = JSON.parse(ev.data);
          if (data.monitor) applyMonitor(data.monitor, true);
        } catch {
          /* ignore */
        }
      };
      sse.onerror = () => {
        if (sse) sse.close();
        sse = null;
        window.ForgeHud.log("GPU stream dropped — polling /monitor", "warn");
        pollOnce();
      };
    } catch {
      sse = null;
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
      utilHistory.clear();
      renderLogs();
    },
  };

  async function start() {
    ensureHud();
    apiBase = await resolveBase();
    window.ForgeHud.log(
      apiBase
        ? "Forge HUD: live GPU stream (nvtop-class NVML)"
        : "Forge HUD: API not found",
      apiBase ? "ok" : "warn"
    );
    await refreshFleetCache();
    connectGpuStream();
    pollOnce();
    setInterval(pollOnce, POLL_MS);

    if (!pingTimer) {
      pingTimer = setInterval(async () => {
        const hud = document.getElementById("forge-gpu-hud");
        if (!hud || !apiBase) return;
        const llm = hud.querySelector(".forge-hud-llm");
        if (llm) llm.dataset.pingPending = "1";
        lastPing = await fetchPing();
        delete llm?.dataset.pingPending;
      }, 45000);
      lastPing = await fetchPing();
    }
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", start);
  } else {
    start();
  }
})();
