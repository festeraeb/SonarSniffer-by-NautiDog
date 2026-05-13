import sqlite3
con = sqlite3.connect('db/wrecks.db')

print('=== source values in features ===')
for r in con.execute("SELECT source, COUNT(*) FROM features GROUP BY source ORDER BY 2 DESC LIMIT 20"):
    print(r)

print()
print('=== mag features lat/lon range ===')
for r in con.execute("SELECT MIN(latitude),MAX(latitude),MIN(longitude),MAX(longitude),COUNT(*) FROM features WHERE mag_mean IS NOT NULL AND latitude IS NOT NULL"):
    print(r)

print()
print('=== features with training_confidence and coords ===')
for r in con.execute("""SELECT latitude, longitude, name, training_confidence, magnetic_weight, mag_tmi_peak, mag_label
    FROM features
    WHERE training_confidence IS NOT NULL AND latitude IS NOT NULL AND longitude IS NOT NULL
    AND NOT (latitude=45.0 AND longitude=-83.0)
    ORDER BY training_confidence DESC LIMIT 10"""):
    print(r)

print()
print('=== masking_candidates survey_id distribution ===')
for r in con.execute("SELECT survey_id, COUNT(*), MIN(center_lat), MAX(center_lat), MIN(center_lon), MAX(center_lon) FROM masking_candidates GROUP BY survey_id ORDER BY 2 DESC LIMIT 15"):
    print(r)

con.close()
