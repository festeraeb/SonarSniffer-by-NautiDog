# Collab scrapes

## Canonical Google URL (friend prepped context)

See `../COLLAB_GOOGLE_URL.txt` — use with:

- **Cursor Browser MCP** (logged-in session): navigate, snapshot, save here.
- **Playwright headless** (`scripts/forge_collab/scrape_google_context.mjs`): often hits Google CAPTCHA; use headed mode on a machine where you are signed in.

```bash
# Headed scrape (you complete CAPTCHA once if needed)
COLLAB_GOOGLE_URL="$(head -1 var/forge_collab/COLLAB_GOOGLE_URL.txt)" \
  node scripts/forge_collab/scrape_google_context.mjs
# Or set HEADLESS=0 in script if we add it
```

## Files

| File | Source |
|------|--------|
| `google_straits_bag_survey.txt` | Playwright (may be CAPTCHA page) |
| `google_straits_bag_survey.png` | Screenshot |
| `mackinac_survey_context.md` | Cursor synthesis from NCEI + 1992 bottomland report |

After scrape succeeds, run:

```bash
python3 scripts/forge_collab/ingest_scrape_to_proposal.py
```
