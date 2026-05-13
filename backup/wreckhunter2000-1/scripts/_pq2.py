import sqlite3, json
con = sqlite3.connect('db/scan_queue.db')
cols = [c[1] for c in con.execute('PRAGMA table_info(scan_jobs)')]
rows = con.execute("SELECT * FROM scan_jobs ORDER BY status, created_at").fetchall()
for r in rows:
    d = dict(zip(cols, r))
    p = json.loads(d.get('params') or '{}')
    print(d['status'], d['id'][:8], d['label'])
    print('  mission:', p.get('mission',''), '| date_start:', p.get('date_start',''), '| date_end:', p.get('date_end',''))
    print('  description:', p.get('description',''))
    print('  sub_zone:', p.get('sub_zone',''), '| passes:', p.get('passes',''))
    print()
