import sqlite3
con = sqlite3.connect('db/scan_queue.db')
r = con.execute("UPDATE scan_jobs SET status='QUEUED', started_at=NULL, worker_id=NULL, error_msg=NULL WHERE status IN ('FAILED','RUNNING')")
con.commit()
print('Reset', r.rowcount, 'jobs back to QUEUED')
r2 = con.execute("SELECT status, COUNT(*) FROM scan_jobs GROUP BY status")
for row in r2: print(' ', row[0], row[1])
