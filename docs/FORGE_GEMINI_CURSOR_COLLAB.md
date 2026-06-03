# Forge collab — Gemini ↔ Cursor (no clipboard)

You should not shuttle paragraphs between chat UIs. Use a **shared bus** in this repo plus optional browser/Notion MCP.

## Roles

| Brain | Strength | Must not |
|-------|----------|----------|
| **Gemini** | Literature, survey IDs, hypothesis, metric gates | Invent `knobs.pon.rs`, fake `cargo` flags, repo paths |
| **Cursor** | Codebase truth, Rust, Forge wiring, `sat-run` | Guess knob names without reading `types.rs` / `poc.rs` |

**Playwright (browser MCP + headless script):** Pinned Google URL in `var/forge_collab/COLLAB_GOOGLE_URL.txt` (friend’s prepped AI context). Cursor reads that page when signed in; headless script may hit CAPTCHA — use `HEADLESS=0` on your desktop.

```bash
# Re-scrape when logged into Google (headed)
HEADLESS=0 node scripts/forge_collab/scrape_google_context.mjs "$(head -1 var/forge_collab/COLLAB_GOOGLE_URL.txt)"
python3 scripts/forge_collab/ingest_scrape_to_proposal.py
python3 scripts/forge_collab/process_collab_inbox.py
```

**Pushback is required.** Cursor `status: pushback` when Gemini conflicts with repo. Gemini replies with `status: confirm|revise` on Cursor fixes.

## Bus layout

```text
var/forge_collab/
  inbox/gemini_proposal.json      ← Gemini writes (one paste OR API script)
  outbox/cursor_response.json     ← Cursor writes
  inbox/gemini_ack.json           ← Gemini replies to Cursor (one paste OR API)
  merged/round_N_decision.json    ← Cursor merges accepted plan
  scrapes/                        ← Playwright captures (optional)
```

## Gemini proposal schema

See `scripts/forge_collab/gemini_proposal.schema.json`.

## Round loop

1. Gemini fills `inbox/gemini_proposal.json` (you paste once into that file, or run `ingest_gemini_brief.sh`).
2. You say in Cursor: **"Process collab inbox"** — agent runs `process_collab_inbox.py`, writes `outbox/cursor_response.json`.
3. **Preferred:** With your Google AI thread open in **Cursor Browser** (signed in), say *“post status to Gemini”* — agent pastes into **Ask anything** and scrapes the JSON reply into `inbox/gemini_ack.json`.
4. **Fallback:** Copy/paste `outbox/GEMINI_PASTE_OR_UPLOAD.md` into Gemini manually if browser MCP is unavailable.

### Browser MCP rules (important)

- **Do not `browser_navigate`** on the friend’s Google AI thread unless it is truly closed — reload is slow and **drops session context**.
- **Long payloads:** split and send **one part after another** in the **same tab** (multiple messages), not one 25 KB paste.
  ```bash
  python3 scripts/forge_collab/mk_cdp_paste_chunks.py var/forge_collab/outbox/GEMINI_FULL_CONTEXT_ROUND7.md
  # → /tmp/cdp_paste_chunks/chunk_01.json … ; agent: browser_lock → browser_cdp each → unlock
  ```
- **Short payloads (~3 KB):** single paste via `mk_cdp_paste_expr.py` + one `browser_cdp` + send.
- **Read replies:** scrape with `browser_cdp` `Runtime.evaluate` on the same tab (brace-match JSON) → `inbox/gemini_ack_roundN.json` — user is not the middleman.
5. Run `python3 scripts/forge_collab/merge_gemini_ack.py` (or say “process collab ack”).
5. Cursor runs `merge_collab_round.py` → job JSON + code + `implementor_learnings.jsonl`.
6. Optional: Playwright verifies NCEI/Forge URLs listed in proposal `references`.

## What you stop doing

- Pasting Mixtral/Gemma outputs into Gemini.
- Pasting Gemini essays into Cursor.
- Copying Google AI Mode threads.

## What you might still do once per round

- Save Gemini's JSON block into `inbox/gemini_proposal.json` **or** use Notion MCP page both agents read.

Full automation: `GEMINI_API_KEY` + `scripts/forge_collab/poll_gemini.py` (optional, not required).
