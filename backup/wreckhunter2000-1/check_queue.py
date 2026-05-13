import sqlite3, sys, os

db = os.path.join(os.path.dirname(__file__), "db", "scan_queue.db")
conn = sqlite3.connect(db)

tables = [t[0] for t in conn.execute("SELECT name FROM sqlite_master WHERE type=?", ("table",)).fetchall()]
print("Tables:", tables)

for tbl in tables:
    cols = [c[1] for c in conn.execute(f"PRAGMA table_info({tbl})").fetchall()]
    count = conn.execute(f"SELECT COUNT(*) FROM {tbl}").fetchone()[0]
    print(f"\n  {tbl} ({count} rows) cols={cols}")
    rows = conn.execute(f"SELECT * FROM {tbl} ORDER BY rowid DESC LIMIT 10").fetchall()
    for r in rows:
        print("   ", r)

conn.close()
