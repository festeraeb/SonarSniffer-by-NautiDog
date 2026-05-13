import sqlite3, os
from pathlib import Path

db = Path("Z:/LAKE_MICHIGAN_CENSUS_2026.db")
print(f"DB size: {db.stat().st_size:,} bytes")

conn = sqlite3.connect(str(db))
tables = conn.execute("SELECT name FROM sqlite_master WHERE type='table'").fetchall()
print(f"Tables: {tables}")
for (t,) in tables:
    try:
        n = conn.execute(f"SELECT COUNT(*) FROM [{t}]").fetchone()[0]
        print(f"  {t}: {n} rows")
    except Exception as e:
        print(f"  {t}: {e}")
conn.close()
