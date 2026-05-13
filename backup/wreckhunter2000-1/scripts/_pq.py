import sqlite3, json
con = sqlite3.connect('db/scan_queue.db')
rows = con.execute("SELECT id,label,sensors,status FROM scan_jobs ORDER BY status,created_at").fetchall()
for r in rows:
    print(r[3], r[0][:8], r[1], r[2])
