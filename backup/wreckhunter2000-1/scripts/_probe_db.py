import sqlite3, json, os
from pathlib import Path
con = sqlite3.connect('db/wrecks.db')

print('=== ALL TABLES ===')
for r in con.execute("SELECT name FROM sqlite_master WHERE type='table'"):
    cols = [c[1] for c in con.execute(f'PRAGMA table_info([{r[0]}])')]
    cnt = con.execute(f'SELECT COUNT(*) FROM [{r[0]}]').fetchone()[0]
    print(f'  {r[0]}: {cnt} rows | {cols[:15]}')

print()
print('=== MAG columns in features (non-null counts) ===')
mag_cols = ['mag_mean','mag_std','mag_max','mag_label','mag_as_peak','mag_vd_peak','mag_tmi_peak',
            'mag_spike_w_m','mag_zco_km','mag_polarity','magnetic_weight','magnetic_potential','training_confidence']
for c in mag_cols:
    n = con.execute(f'SELECT COUNT(*) FROM features WHERE [{c}] IS NOT NULL').fetchone()[0]
    if n > 0:
        print(f'  {c}: {n}')

print()
print('=== features with mag data sample ===')
rows = con.execute('''SELECT name,latitude,longitude,mag_mean,mag_tmi_peak,mag_label,
    magnetic_weight,training_confidence,is_steel_freighter
    FROM features WHERE mag_mean IS NOT NULL LIMIT 3''').fetchall()
for row in rows: print(row)

print()
print('=== training_confidence distribution ===')
for r in con.execute('''SELECT
  CASE WHEN training_confidence>=0.9 THEN ">=0.9"
       WHEN training_confidence>=0.8 THEN "0.8-0.9"
       WHEN training_confidence>=0.7 THEN "0.7-0.8"
       WHEN training_confidence>=0.5 THEN "0.5-0.7"
       ELSE "<0.5" END bucket, COUNT(*) n
  FROM features WHERE training_confidence IS NOT NULL GROUP BY 1 ORDER BY MIN(training_confidence) DESC'''):
    print(r)

print()
print('=== mag_label values ===')
for r in con.execute("SELECT mag_label, COUNT(*) FROM features WHERE mag_label IS NOT NULL GROUP BY mag_label"):
    print(r)

con.close()
