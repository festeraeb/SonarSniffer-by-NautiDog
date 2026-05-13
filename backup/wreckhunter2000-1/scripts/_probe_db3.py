import sqlite3
con = sqlite3.connect('db/wrecks.db')

print('=== ThunderBay_NOAA features ===')
cols = [c[1] for c in con.execute('PRAGMA table_info(features)')]
rows = con.execute("""
    SELECT name, latitude, longitude, coord_quality, found_status, feature_type, depth, mag_mean
    FROM features WHERE source='ThunderBay_NOAA'
    ORDER BY latitude""").fetchall()
print(f"Count: {len(rows)}")
for r in rows[:15]:
    print(r)

print()
print('=== coord_quality distribution in ThunderBay_NOAA ===')
for r in con.execute("SELECT coord_quality, COUNT(*) FROM features WHERE source='ThunderBay_NOAA' GROUP BY coord_quality"):
    print(r)

print()
print('=== Swayze sources ===')
for r in con.execute("SELECT source, COUNT(*) FROM features GROUP BY source"):
    print(r)

print()
print('=== coord_quality all sources ===')
for r in con.execute("SELECT coord_quality, COUNT(*) FROM features GROUP BY coord_quality ORDER BY 2 DESC"):
    print(r)

con.close()
