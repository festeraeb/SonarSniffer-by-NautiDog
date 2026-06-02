/** Probe public endpoints from the IONOS front-end. LAN-only links are skipped. */
const PUBLIC_CHECKS = [
  { id: "forge-api", url: "https://api.cesarops.org/health", expect: 200 },
  { id: "app", url: "https://app.cesarops.org/", expect: 200 },
];

async function probeOne(check) {
  const el = document.querySelector(`[data-probe="${check.id}"]`);
  if (!el) return;
  el.className = "status-dot pending";
  try {
    const res = await fetch(check.url, { mode: "cors", cache: "no-store" });
    el.className = res.ok || res.status === check.expect ? "status-dot ok" : "status-dot fail";
    el.title = `${check.url} → ${res.status}`;
  } catch (err) {
    el.className = "status-dot fail";
    el.title = `${check.url} → ${err.message || "unreachable"}`;
  }
}

export function runPublicHealthChecks() {
  PUBLIC_CHECKS.forEach(probeOne);
}

async function probeForgeService() {
  const bases = [
    "https://api.cesarops.org",
    "http://10.0.0.61:9100",
    "http://100.72.182.77:9100",
  ];
  const el = document.querySelector('[data-probe="forge-api"]');
  if (!el) return;
  el.className = "status-dot pending";
  for (const base of bases) {
    try {
      const res = await fetch(`${base}/health`, { mode: "cors", cache: "no-store" });
      const j = await res.json();
      if (j?.service === "cesarops-forge-v2") {
        el.className = "status-dot ok";
        el.title = `${base}/health → forge-v2`;
        return;
      }
    } catch {
      /* try next */
    }
  }
  el.className = "status-dot fail";
  el.title = "Forge API not reachable (check tunnel or LAN)";
}

document.addEventListener("DOMContentLoaded", () => {
  runPublicHealthChecks();
  probeForgeService();
  const btn = document.getElementById("refresh-health");
  if (btn) {
    btn.addEventListener("click", () => {
      runPublicHealthChecks();
      probeForgeService();
    });
  }
});
