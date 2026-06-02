/**
 * Load fleet health poll state for n8n Code nodes (embed in workflow JSON).
 * Source of truth: REPO/var/fleet-health/latest.json (from fleet_health_poll.sh)
 */
function loadFleetHealthState(repo) {
  const fs = require('fs');
  const path = require('path');
  const base = repo || '/data/codebase/repos/wreckhunter2000-1';
  const candidates = [
    path.join(base, 'var/fleet-health/latest.json'),
    '/tmp/fleet_health_latest.json',
    '/tmp/fleet_route_health_last.json',
  ];
  for (const p of candidates) {
    try {
      if (fs.existsSync(p)) {
        const raw = JSON.parse(fs.readFileSync(p, 'utf8'));
        if (raw.endpoints) return raw;
        if (raw.healthy !== undefined) return { endpoints: {}, probe: raw };
      }
    } catch (e) { /* try next */ }
  }
  return { endpoints: {}, n8n_url: 'http://127.0.0.1:5678' };
}

function endpointForRole(state, role, fallback) {
  const ep = (state.endpoints || {})[role];
  if (ep && ep.url) return ep.url;
  return fallback || '';
}

function n8nUrl(state) {
  return state.n8n_url || state.n8n_bridge || 'http://127.0.0.1:5678';
}

module.exports = { loadFleetHealthState, endpointForRole, n8nUrl };
