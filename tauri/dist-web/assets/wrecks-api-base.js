/** Shared API base discovery for wreck tools. */
const API_BASES = [
  "https://api.cesarops.org",
  "http://127.0.0.1:8099",
  "http://10.0.0.61:8099",
];

export async function pickApiBase() {
  for (const base of API_BASES) {
    try {
      const r = await fetch(`${base}/stats`, { signal: AbortSignal.timeout(8000) });
      if (r.ok) return base;
    } catch {
      /* next */
    }
  }
  return null;
}

export { API_BASES };
