# Historic news → wreck re-estimation → drift analysis

Improve **estimated** Swayze pins using Chronicling America (LOC API), then queue cases for drift tools.

## Pipeline

```text
1. harvest_chronicling_america.py   → articles.jsonl, mentions.jsonl, chunks.jsonl
2. reestimate_from_historic_news.py → reestimated_wrecks.json, drift_candidates.json
3. historical_drift.py              → forward/backward cones from candidate seeds
```

## Step 1 — Harvest (LOC API)

Uses `https://www.loc.gov/collections/chronicling-america/?fo=json` with:

- `location_state`: Michigan, Wisconsin, Ohio, Illinois, Indiana, New York, Pennsylvania, Minnesota
- Keywords: gale, schooner, steamer, "lost with all hands", foundered, overdue, etc.
- Retrospective queries: "disasters on the lakes since…" (early wrecks via 1800s compilations)
- Port-scoped queries: Detroit, Chicago, Cleveland, Buffalo, …

```bash
pip install requests
python3 scripts/harvest_chronicling_america.py --quick          # smoke test (~4 states, few queries)
python3 scripts/harvest_chronicling_america.py --retrospective  # include early-wreck compilations
python3 scripts/harvest_chronicling_america.py --states michigan,wisconsin,ohio --max-pages 5
```

Outputs: `outputs/historic_news/`

## Step 2 — Re-estimate & drift candidates

Matches vessel names in mentions to `features` rows with `pin_class` estimated/parsed.  
If newspaper text geocodes to a **better** place (gazetteer + "off Whitefish Point" parsing), proposes new lat/lon (`coord_quality: historic_news`).

```bash
python3 scripts/reestimate_from_historic_news.py
python3 scripts/reestimate_from_historic_news.py --apply   # log to historic_news_mentions table
```

## Step 3 — RTX 2060 batch + save to database (cesarops2)

Resumable batches (10–20 wrecks) using **llama-server :5200** on the 2060:

```bash
bash scripts/cesarops2_research_lab.sh start
bash scripts/run_wreck_news_batch_c2.sh
# or
python3 scripts/batch_wreck_historic_news.py --resume --batch-size 15 --apply-coords
```

Checkpoint: `outputs/historic_news/batch_llm_state.json`

**Database tables** (in `wrecks.db`):

| Table | Contents |
|-------|----------|
| `historic_news_articles` | Good matched articles (URL, title, date, OCR text) |
| `historic_news_matches` | LLM verdict, confidence, estimated lat/lon, drift flag |

Strong matches (`--apply-coords`) update `features` with `coord_quality=historic_news_llm`.

## Step 4 — Vector index (optional)

`chunks.jsonl` rows are ready for embedding:

```json
{"id": "chnk_…", "text": "…", "metadata": {"vessel_name", "article_date", "lat", "lon", …}}
```

Embed with OpenAI `text-embedding-3-small` or local model → Pinecone / Milvus / sqlite-vec.

## Supplementary sources (manual / future)

| Source | URL |
|--------|-----|
| Digital Michigan Newspaper Portal | https://digmichnews.cmich.edu/ |
| Maritime History of the Great Lakes | https://www.maritimehistoryofthegreatlakes.ca/ |

## Drift analysis

High-priority rows in `drift_candidates.json` include `departure` / `anchors` for `pipelines/satellite/historical_drift.py`.

Do **not** treat re-estimated pins as survey-verified until drift + independent confirmation.
