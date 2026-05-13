import sqlite3
conn = sqlite3.connect("db/scan_queue.db")
# Reset any jobs marked with dry_run result or stuck in RUNNING/DONE with no real output
n1 = conn.execute(
    "UPDATE scan_jobs SET status='QUEUED', started_at=NULL, finished_at=NULL, "
    "result_path=NULL, error_msg=NULL, worker_id=NULL "
    "WHERE result_path='dry_run'"
).rowcount
n2 = conn.execute(
    "UPDATE scan_jobs SET status='QUEUED', started_at=NULL, finished_at=NULL, "
    "result_path=NULL, error_msg=NULL, worker_id=NULL "
    "WHERE status IN ('RUNNING','FAILED') AND result_path IS NULL"
).rowcount
conn.commit()
rows = conn.execute("SELECT status, COUNT(*) as n FROM scan_jobs GROUP BY status").fetchall()
print(f"Reset dry_run: {n1}, reset stuck: {n2}")
for r in rows:
    print(f"  {r[0]}: {r[1]}")
conn.close()
