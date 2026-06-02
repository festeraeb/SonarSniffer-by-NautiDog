/**
 * Client-side gate for fleet ops pages (tools / health monitor).
 * Front page links with ?pw=8106940539 only — not secret, keeps casual visitors out.
 */
const OPS_ACCESS = "8106940539";

export function hasOpsAccess() {
  return new URLSearchParams(location.search).get("pw") === OPS_ACCESS;
}

export function opsUrl(path) {
  const base = path.startsWith("/") ? path : path;
  const sep = base.includes("?") ? "&" : "?";
  return `${base}${sep}pw=${OPS_ACCESS}`;
}

/** Call at top of gated pages; returns false and replaces body if denied. */
export function requireOpsGate(pageTitle = "Fleet operations") {
  if (hasOpsAccess()) return true;

  document.body.innerHTML = `
    <main class="hero" style="min-height:60vh;display:flex;flex-direction:column;justify-content:center;align-items:center;text-align:center">
      <h1>${pageTitle}</h1>
      <p class="lead">This page is for fleet operators only. Use the link from the CESARops home page.</p>
      <a class="btn btn-primary" href="index.html">← Back to home</a>
    </main>
  `;
  return false;
}
