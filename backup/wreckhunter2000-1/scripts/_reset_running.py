import sqlite3, os, sys
db = os.path.join(os.path.dirname(__file__), '..', 'db', 'scan_queue.db')
conn = sqlite3.connect(db)
cur = conn.cursor()

statuses = ["RUNNING"]
if "--failed" in sys.argv:
    statuses.append("FAILED")

for st in statuses:
    cur.execute(f"UPDATE scan_jobs SET status='QUEUED', started_at=NULL, finished_at=NULL, worker_id=NULL, error_msg=NULL WHERE status='{st}'")
    print(f"Reset {cur.rowcount} {st} jobs back to QUEUED")

conn.commit()
cur.execute("SELECT status, COUNT(*) FROM scan_jobs GROUP BY status")
for row in cur.fetchall():
    print(f"  {row[0]}: {row[1]}")
conn.close()
