/**
 * Great Lakes wreck map — selectable layers by trust level.
 */
import { pickApiBase } from "./wrecks-api-base.js";

const LAYER_DEFS = {
  verified: {
    label: "Verified / survey GPS",
    color: "#22c55e",
    fillColor: "#22c55e",
    defaultOn: true,
  },
  estimated: {
    label: "Estimated loss site",
    color: "#f59e0b",
    fillColor: "#f59e0b",
    defaultOn: true,
  },
  parsed: {
    label: "Swayze parsed coords",
    color: "#94a3b8",
    fillColor: "#64748b",
    defaultOn: false,
  },
  canonical: {
    label: "Best pin per wreck (deduped)",
    color: "#00d4ff",
    fillColor: "#00d4ff",
    defaultOn: false,
  },
};

let map;
let layerGroups = {};
let apiBase = "";

function el(id) {
  return document.getElementById(id);
}

function activeLayers() {
  return Object.keys(LAYER_DEFS).filter((k) => {
    const cb = el(`layer-${k}`);
    return cb && cb.checked;
  });
}

function bboxParam() {
  if (!map) return {};
  const b = map.getBounds();
  return {
    min_lat: b.getSouth().toFixed(5),
    max_lat: b.getNorth().toFixed(5),
    min_lon: b.getWest().toFixed(5),
    max_lon: b.getEast().toFixed(5),
  };
}

function popupHtml(props) {
  const lines = [
    `<strong>${escapeHtml(props.name || "Unknown")}</strong>`,
    `<span class="pin-tag pin-${props.pin_class || props.layer}">${props.pin_class || props.layer}</span>`,
  ];
  if (props.coord_quality) lines.push(`Quality: ${escapeHtml(props.coord_quality)}`);
  if (props.depth) lines.push(`Depth: ${escapeHtml(String(props.depth))}`);
  if (props.preserve) lines.push(`Preserve: ${escapeHtml(props.preserve)}`);
  if (props.member_count) lines.push(`Cluster: ${props.member_count} records`);
  if (props.source) {
    const s = String(props.source);
    lines.push(
      s.startsWith("http")
        ? `<a href="${s}" target="_blank" rel="noopener">Source</a>`
        : `Source: ${escapeHtml(s.slice(0, 60))}`,
    );
  }
  return lines.join("<br>");
}

function escapeHtml(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

function propsRadius(props) {
  if (props.layer === "canonical" || props.origin === "canonical") return 7;
  if (props.pin_class === "verified") return 6;
  if (props.pin_class === "estimated") return 5;
  return 4;
}

async function loadGeoJson() {
  const layers = activeLayers();
  if (!layers.length) {
    Object.values(layerGroups).forEach((g) => g.clearLayers());
    el("map-status").textContent = "Select at least one layer.";
    return;
  }

  el("map-status").textContent = "Loading pins…";
  const params = new URLSearchParams({
    layers: layers.join(","),
    limit_per_layer: "5000",
    ...bboxParam(),
  });

  const r = await fetch(`${apiBase}/wrecks/map/geojson?${params}`);
  if (!r.ok) throw new Error(`map ${r.status}`);
  const geo = await r.json();

  Object.keys(layerGroups).forEach((k) => layerGroups[k].clearLayers());

  const counts = { verified: 0, estimated: 0, parsed: 0, canonical: 0, unknown: 0 };

  for (const f of geo.features || []) {
    const p = f.properties || {};
    const layerKey =
      p.layer === "canonical"
        ? "canonical"
        : p.pin_class && LAYER_DEFS[p.pin_class]
          ? p.pin_class
          : p.layer && LAYER_DEFS[p.layer]
            ? p.layer
            : "unknown";
    if (!layerGroups[layerKey]) continue;
    counts[layerKey] = (counts[layerKey] || 0) + 1;

    const def = LAYER_DEFS[layerKey];
    const [lon, lat] = f.geometry.coordinates;
    const marker = L.circleMarker([lat, lon], {
      radius: propsRadius(p),
      color: def.color,
      fillColor: def.fillColor,
      fillOpacity: layerKey === "estimated" ? 0.55 : 0.85,
      weight: 2,
    });
    marker.bindPopup(popupHtml(p));
    layerGroups[layerKey].addLayer(marker);
  }

  const parts = Object.entries(counts)
    .filter(([, n]) => n > 0)
    .map(([k, n]) => `${k}: ${n}`);
  el("map-status").textContent = `${geo.meta?.count ?? 0} pins (${parts.join(" · ")})`;
}

let reloadTimer;
function scheduleReload() {
  clearTimeout(reloadTimer);
  reloadTimer = setTimeout(() => loadGeoJson().catch((e) => {
    el("map-status").textContent = `Error: ${e.message}`;
  }), 300);
}

export async function initWrecksMap() {
  apiBase = await pickApiBase();
  if (!apiBase) {
    el("map-status").textContent = "API unreachable — map needs wrecks-api on :8099 or api.cesarops.org";
    return;
  }

  map = L.map("wreck-map", {
    center: [44.5, -84.0],
    zoom: 6,
    minZoom: 5,
    maxZoom: 14,
  });

  L.tileLayer("https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png", {
    attribution: "&copy; OpenStreetMap",
    maxZoom: 18,
  }).addTo(map);

  for (const [key, def] of Object.entries(LAYER_DEFS)) {
    layerGroups[key] = L.layerGroup();
    if (def.defaultOn) {
      layerGroups[key].addTo(map);
    }
    const cb = el(`layer-${key}`);
    if (cb) {
      cb.checked = def.defaultOn;
      cb.addEventListener("change", () => {
        if (cb.checked) {
          layerGroups[key].addTo(map);
        } else {
          map.removeLayer(layerGroups[key]);
        }
        scheduleReload();
      });
    }
  }

  map.on("moveend", scheduleReload);
  el("map-reload")?.addEventListener("click", scheduleReload);

  await loadGeoJson();
}

initWrecksMap();
