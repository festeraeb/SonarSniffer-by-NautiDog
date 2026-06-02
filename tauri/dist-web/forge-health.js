/**
 * Live Forge cluster health: GPU temps/VRAM/util/power, peak TFLOPS estimates, LLM tok/s.
 * Picks the first reachable Forge API (public tunnel, LAN, or Tailscale).
 */

const FORGE_CANDIDATES = [
  "https://api.cesarops.org",
  "http://10.0.0.61:9100",
  "http://100.72.182.77:9100",
];

const AUGMENT_STATUS = "http://10.0.0.201:5500/status";

/** Approximate FP16 tensor-core peak TFLOPS by GPU name (spec sheet). */
const PEAK_TFLOPS_FP16 = [
  [/V100.*32/i, 125],
  [/V100/i, 112],
  [/A100/i, 312],
  [/P100/i, 18.7],
  [/RTX 4090/i, 330],
  [/RTX 3090/i, 142],
  [/2060\s*SUPER/i, 44],
  [/2060/i, 40],
  [/2070/i, 60],
  [/2080/i, 60],
  [/GTX 1070/i, 19.3],
  [/GTX 1080/i, 22.3],
  [/P106/i, 3.8],
  [/T4/i, 65],
];

function peakTflops(gpuName) {
  if (!gpuName) return null;
  for (const [re, t] of PEAK_TFLOPS_FP16) {
    if (re.test(gpuName)) return t;
  }
  return null;
}

function effectiveTflops(peak, utilPct) {
  if (peak == null || utilPct == null) return null;
  return (peak * (utilPct / 100)).toFixed(1);
}

function tempClass(c) {
  if (c >= 85) return "temp-hot";
  if (c >= 75) return "temp-warm";
  return "temp-ok";
}

function vramPct(used, total) {
  if (!total) return 0;
  return Math.min(100, Math.round((used / total) * 100));
}

async function fetchJson(url, timeoutMs = 12000) {
  const ctrl = new AbortController();
  const t = setTimeout(() => ctrl.abort(), timeoutMs);
  try {
    const res = await fetch(url, { mode: "cors", cache: "no-store", signal: ctrl.signal });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return await res.json();
  } finally {
    clearTimeout(t);
  }
}

async function isForgeBase(base) {
  try {
    const j = await fetchJson(`${base.replace(/\/$/, "")}/health`, 8000);
    return j && j.service === "cesarops-forge-v2";
  } catch {
    return false;
  }
}

export async function resolveForgeBase() {
  const saved = localStorage.getItem("forge_api_base");
  if (saved && (await isForgeBase(saved))) return saved.replace(/\/$/, "");

  for (const base of FORGE_CANDIDATES) {
    if (await isForgeBase(base)) {
      localStorage.setItem("forge_api_base", base);
      return base.replace(/\/$/, "");
    }
  }
  return null;
}

function renderGpuCard(gpu, hostLabel) {
  const peak = peakTflops(gpu.name);
  const eff = effectiveTflops(peak, gpu.utilization_pct);
  const vram = vramPct(gpu.memory_used_mb ?? gpu.vram_used_mb, gpu.memory_total_mb ?? gpu.vram_total_mb);
  const temp = gpu.temperature_c ?? 0;
  const util = gpu.utilization_pct ?? 0;
  const power = gpu.power_draw_w != null ? `${gpu.power_draw_w.toFixed(0)} W` : "—";
  const usedMb = gpu.memory_used_mb ?? gpu.vram_used_mb ?? 0;
  const totalMb = gpu.memory_total_mb ?? gpu.vram_total_mb ?? 0;

  return `
    <article class="gpu-card">
      <header class="gpu-card-head">
        <span class="gpu-host">${hostLabel}</span>
        <span class="gpu-index">GPU ${gpu.index ?? gpu.id ?? "?"}</span>
      </header>
      <h4 class="gpu-name">${gpu.name || "Unknown GPU"}</h4>
      <div class="gpu-metrics">
        <div class="metric"><span class="metric-label">Temp</span><span class="metric-value ${tempClass(temp)}">${temp}°C</span></div>
        <div class="metric"><span class="metric-label">Util</span><span class="metric-value">${util}%</span></div>
        <div class="metric"><span class="metric-label">Power</span><span class="metric-value">${power}</span></div>
        <div class="metric"><span class="metric-label">Peak FP16</span><span class="metric-value">${peak != null ? `~${peak} TFLOPS` : "—"}</span></div>
        <div class="metric"><span class="metric-label">Est. live</span><span class="metric-value">${eff != null ? `~${eff} TFLOPS` : "idle"}</span></div>
      </div>
      <div class="vram-bar" title="VRAM ${usedMb} / ${totalMb} MB">
        <div class="vram-fill" style="width:${vram}%"></div>
      </div>
      <div class="vram-label">${usedMb} / ${totalMb} MB VRAM (${vram}%)</div>
    </article>
  `;
}

function renderEngineRow(slot) {
  if (!slot) return "";
  const label = slot.label || slot.role || "engine";
  const online = slot.online;
  const tps = slot.tps != null ? `${Number(slot.tps).toFixed(1)} tok/s` : "—";
  const dot = online ? "ok" : "fail";
  const role = slot.role ? `<span class="forge-muted">${slot.role}</span> ` : "";
  return `
    <div class="engine-row">
      <span class="status-dot ${dot}"></span>
      <div class="engine-meta">
        <div class="engine-name">${role}${label}</div>
        <div class="tool-url">${slot.url || ""}</div>
      </div>
      <span class="engine-tps">${online ? tps : slot.tps == null && online ? "ping" : "offline"}</span>
    </div>
  `;
}

function renderRoleRows(ping) {
  if (!ping?.roles) {
    return (
      renderEngineRow(ping?.main_engine ? { ...ping.main_engine, role: "coder", label: "coder" } : null) +
      renderEngineRow(
        ping?.p1000_validator ? { ...ping.p1000_validator, role: "reviewer", label: "validator" } : null,
      )
    );
  }
  const order = ["coder", "reviewer", "thinker", "draft"];
  let html = "";
  for (const key of order) {
    const slot = ping.roles[key];
    if (slot) html += renderEngineRow(slot);
  }
  return html;
}

function renderDiscover(nodes) {
  if (!Array.isArray(nodes) || nodes.length === 0) {
    return '<p class="forge-muted">No cluster nodes discovered.</p>';
  }
  return nodes
    .slice(0, 12)
    .map((n) => {
      const online = n.online ?? n.reachable;
      const dot = online ? "ok" : "fail";
      const model = n.model || n.last_model || "—";
      const port = n.port != null ? `:${n.port}` : "";
      return `<div class="engine-row">
        <span class="status-dot ${dot}"></span>
        <div class="engine-meta">
          <div class="engine-name">${n.name || n.host || "node"}</div>
          <div class="tool-url">${model}${port}</div>
        </div>
      </div>`;
    })
    .join("");
}

function setBanner(el, ok, text) {
  if (!el) return;
  el.className = ok ? "forge-banner ok" : "forge-banner fail";
  el.textContent = text;
}

function gpusFromNodeStatus(data, hostLabel) {
  const cards = [];
  if (Array.isArray(data.all_gpus) && data.all_gpus.length) {
    for (const g of data.all_gpus) {
      cards.push(renderGpuCard(
        {
          id: g.id,
          name: g.name,
          temperature_c: g.temperature_c,
          utilization_pct: g.utilization_pct,
          vram_used_mb: g.vram_used_mb,
          vram_total_mb: g.vram_total_mb,
        },
        hostLabel,
      ));
    }
    return cards.join("");
  }
  const g = data.gpu;
  if (g && g.name) {
    return renderGpuCard(
      {
        name: g.name,
        temperature_c: g.temperature_c,
        utilization_pct: g.utilization_pct,
        vram_used_mb: g.vram_used_mb,
        vram_total_mb: g.vram_total_mb,
      },
      hostLabel,
    );
  }
  return "";
}

export async function runForgeHealth() {
  const banner = document.getElementById("forge-health-banner");
  const gpuGrid = document.getElementById("forge-gpu-grid");
  const engines = document.getElementById("forge-engines");
  const discover = document.getElementById("forge-discover");
  const meta = document.getElementById("forge-health-meta");

  if (!gpuGrid) return;

  setBanner(banner, true, "Resolving Forge API…");
  gpuGrid.innerHTML = '<p class="forge-muted">Loading GPU telemetry…</p>';

  const base = await resolveForgeBase();
  if (!base) {
    setBanner(
      banner,
      false,
      "Forge unreachable. On LAN use http://10.0.0.61:9100 — public api.cesarops.org must proxy /monitor and /validate/ping.",
    );
    gpuGrid.innerHTML = "";
    return;
  }

  if (meta) {
    meta.innerHTML = `API: <code>${base}</code> · <a href="${base}/" target="_blank" rel="noopener">open Forge</a>`;
  }

  const parts = await Promise.allSettled([
    fetchJson(`${base}/health`),
    fetchJson(`${base}/monitor`),
    fetchJson(`${base}/validate/ping`, 20000),
    fetchJson(`${base}/cluster/discover`, 25000),
    (async () => {
      try {
        return await fetchJson(AUGMENT_STATUS, 6000);
      } catch {
        return null;
      }
    })(),
  ]);

  const health = parts[0].status === "fulfilled" ? parts[0].value : null;
  const monitor = parts[1].status === "fulfilled" ? parts[1].value : null;
  const ping = parts[2].status === "fulfilled" ? parts[2].value : null;
  const disc = parts[3].status === "fulfilled" ? parts[3].value : null;
  const augment = parts[4].status === "fulfilled" ? parts[4].value : null;

  const ok = health?.status === "ok";
  const gpuCount = monitor?.gpus?.length ?? health?.gpu_count ?? 0;
  setBanner(
    banner,
    ok,
    ok
      ? `Forge online · ${gpuCount} GPU(s) on host · ${health?.mode || "forge-v2"}`
      : "Forge health check failed",
  );

  let html = "";
  if (monitor?.gpus?.length) {
    html += monitor.gpus.map((g) => renderGpuCard(g, g.host || g.node || "fleet")).join("");
  } else if (health?.gpus?.length) {
    html += health.gpus.map((g) => renderGpuCard(g, "T440")).join("");
  }

  if (augment) {
    const augHtml = gpusFromNodeStatus(augment, "cesarops2 augment");
    if (augHtml) html += augHtml;
  }

  if (!html) {
    html = `<p class="forge-muted">${monitor?.gpu_error || "No GPU metrics (nvidia-smi unavailable on Forge host)."}</p>`;
  }

  gpuGrid.innerHTML = html;

  if (engines) {
    const benchNote = ping?.bench_ran === false ? " (liveness only — tok/s every ~45s)" : "";
    engines.innerHTML =
      `<p class="forge-muted">Live LLM endpoints${benchNote}</p>` + renderRoleRows(ping);
  }

  if (discover) {
    const nodes = Array.isArray(disc) ? disc : disc?.nodes;
    discover.innerHTML = renderDiscover(nodes);
  }
}

document.addEventListener("DOMContentLoaded", () => {
  runForgeHealth();
  const btn = document.getElementById("refresh-health");
  if (btn) {
    btn.addEventListener("click", () => {
      runForgeHealth();
      import("./health.js").then((m) => m.runPublicHealthChecks());
    });
  }
});
