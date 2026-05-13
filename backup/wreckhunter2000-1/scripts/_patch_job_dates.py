"""Patch missing date_start/date_end on all jobs with those labels, any status."""
import sqlite3, json

PATCHES = {
    "NWA_2501_primary":    ("2025-01-01", "2025-03-31"),
    "NWA_2501_extended":   ("2025-01-01", "2025-03-31"),
    "alaska_canopy_areaA": ("2024-05-01", "2024-07-31"),
    "alaska_canopy_areaB": ("2024-05-01", "2024-07-31"),
    "alaska_canopy_areaC": ("2024-05-01", "2024-07-31"),
}

con  = sqlite3.connect("db/scan_queue.db")
cols = [c[1] for c in con.execute("PRAGMA table_info(scan_jobs)")]
rows = con.execute("SELECT * FROM scan_jobs").fetchall()

patched = 0
for raw in rows:
    d      = dict(zip(cols, raw))
    label  = d["label"]
    if label not in PATCHES:
        continue
    params = json.loads(d.get("params") or "{}")
    if params.get("date_start") and params.get("date_end"):
        print(f"  SKIP {label} ({d['status']}) -- already has dates")
        continue
    d0, d1 = PATCHES[label]
    params["date_start"] = d0
    params["date_end"]   = d1
    # also reset to QUEUED so worker picks it up
    con.execute(
        "UPDATE scan_jobs SET params=?, status='QUEUED', started_at=NULL, "
        "worker_id=NULL, error_msg=NULL WHERE id=?",
        (json.dumps(params), d["id"])
    )
    print(f"  PATCHED+RESET {label} ({d['status']}) -> {d0} to {d1}")
    patched += 1

con.commit()
print()
for r in con.execute("SELECT status, COUNT(*) FROM scan_jobs GROUP BY status"):
    print(f"  {r[0]}: {r[1]}")
con.close()
print(f"\nPatched {patched} jobs.")
