# live_reference/

Alternate copies of **live** files (same path under pipelines or repo root), from laptop sources.
Filename pattern: `{mag|satellite|bag}__{file}__SIMILAR_TO_LIVE__{origin}__{basename}`

The `live_key` field in `_MANIFEST.json` is the live file path key (e.g. `pipelines/mag/foo.py`).

**Files:** 7

Suggested P100 task: for each row, `diff -u` live file vs this copy, then merge or reject.
