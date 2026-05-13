#!/usr/bin/env python3
"""
Query wrecks.db and dump verified GPS entries.
"""
import sqlite3, json, pathlib, sys


def resolve_wrecks_db() -> pathlib.Path:
    """Resolve wrecks.db in a repo-friendly order."""
    here = pathlib.Path(__file__).resolve().parent
    candidates = [
        here / "db" / "wrecks.db",
        here.parent / "db" / "wrecks.db",
        here.parent / "Bagrecovery" / "db" / "wrecks.db",
        pathlib.Path(r"C:\Users\thomf\programming\Bagrecovery\db\wrecks.db"),
    ]
    for p in candidates:
        if p.exists():
            return p
    raise FileNotFoundError(
        "Could not find wrecks.db. Checked: " + ", ".join(str(p) for p in candidates)
    )


db_path = resolve_wrecks_db()
print(f"DB: {db_path}  ({db_path.stat().st_size//1024}KB)")

con = sqlite3.connect(str(db_path))
con.row_factory = sqlite3.Row

# Schema
tables = [r[0] for r in con.execute("SELECT name FROM sqlite_master WHERE type='table'").fetchall()]
print("Tables:", tables)
for tn in tables[:8]:
    cols = [r[1] for r in con.execute(f"PRAGMA table_info([{tn}])").fetchall()]
    cnt = con.execute(f"SELECT COUNT(*) FROM [{tn}]").fetchone()[0]
    print(f"  {tn}: {cnt} rows  {cols[:12]}")

print()
# Find the features table (main wreck table)
feat_table = next((t for t in tables if t in ("features","wrecks","vessels","shipwrecks")), tables[0])
print(f"Using table: {feat_table}")

all_cols = [r[1] for r in con.execute(f"PRAGMA table_info([{feat_table}])").fetchall()]
print("All columns:", all_cols)

lat_col = next((c for c in all_cols if "lat" in c.lower()), None)
lon_col = next((c for c in all_cols if "lon" in c.lower()), None)
name_col = next((c for c in all_cols if any(x in c.lower() for x in ["vessel_name","ship_name","name","title"])), None)

print(f"\nlat={lat_col}  lon={lon_col}  name={name_col}")

if lat_col and lon_col:
    total = con.execute(f"SELECT COUNT(*) FROM [{feat_table}]").fetchone()[0]
    # Entries with real coords (non-null, non-zero, non placeholder like 45.0/-83.0)
    has_coords = con.execute(
        f"SELECT COUNT(*) FROM [{feat_table}] WHERE [{lat_col}] IS NOT NULL "
        f"AND [{lat_col}] != 0 AND [{lat_col}] != 45.0 AND [{lon_col}] != -83.0"
    ).fetchone()[0]
    print(f"Total: {total}  With real coords: {has_coords}")

    # Pull them all
    sel_cols = ", ".join(f"[{c}]" for c in all_cols[:20])
    rows = con.execute(
        f"SELECT {sel_cols} FROM [{feat_table}] "
        f"WHERE [{lat_col}] IS NOT NULL AND [{lat_col}] != 0 "
        f"AND [{lat_col}] != 45.0 AND [{lon_col}] != -83.0 "
        f"ORDER BY [{lat_col}]"
    ).fetchall()
    print(f"\nVerified GPS entries ({len(rows)}):")
    for r in rows:
        d = dict(r)
        print(f"  {str(d.get(name_col,'?'))[:40]:40s}  lat={d.get(lat_col)}  lon={d.get(lon_col)}", end="")
        for extra in ["depth_ft","depth_m","lake","material","is_steel","coordinate_source","coordinate_quality","coord_source"]:
            if extra in d and d[extra] is not None:
                print(f"  {extra}={d[extra]}", end="")
        print()

    # Dump as JSON
    out = []
    for r in rows:
        out.append(dict(r))
    outpath = pathlib.Path(__file__).parent / "outputs" / "wrecks_db_verified_coords.json"
    outpath.parent.mkdir(exist_ok=True)
    json.dump(out, open(str(outpath), "w"), indent=2, default=str)
    print(f"\nDumped {len(out)} rows to {outpath}")

con.close()
