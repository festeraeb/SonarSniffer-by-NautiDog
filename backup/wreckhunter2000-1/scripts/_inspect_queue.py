import sqlite3
from pathlib import Path

db = Path('db/scan_queue.db')
if not db.exists():
    print('not found'); exit()
c = sqlite3.connect(str(db))
c.row_factory = sqlite3.Row
tables = [r[0] for r in c.execute("SELECT name FROM sqlite_master WHERE type='table'")]
print('Tables:', tables)
try:
    rows = c.execute('SELECT status, COUNT(*) n FROM scan_jobs GROUP BY status').fetchall()
    for r in rows: print(r['status'], r['n'])
    cols = [d[0] for d in c.execute('SELECT * FROM scan_jobs LIMIT 0').description]
    print('Cols:', cols)
    sample = c.execute('SELECT * FROM scan_jobs ORDER BY created_at DESC LIMIT 5').fetchall()
    for r in sample: print(dict(r))
except Exception as e:
    print('error:', e)
