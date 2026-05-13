for lake in superior straits huron erie ontario;
do
  python scan_cli.py --lake  --dates 2024-01-01 2025-12-31 --download --passes standard_anomaly hydrocarbon stumpf_bathy nauticuvs swir_silt_erasure mussel_clearspot triple_lock --label all_sensors_baseline
done
