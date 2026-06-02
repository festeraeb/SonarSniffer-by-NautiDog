## Mission JSON edits (bbox, gt_wreck_names)

1. Edit the `bbox` parameter in the `straits_local_run.json` file to cover the area of interest around the Robert Burns hypothesis location:

   ```json
   "bbox": [45.84, -84.63, 45.90, -84.54]
   ```

2. Add the wreck names to the `gt_wreck_names` parameter in the `straits_local_run.json` file:

   ```json
   "gt_wreck_names": ["Robert Burns", "Cedarville"]
   ```

## Commands (copy-paste)

1. Run the Cedarville calibration using the following command:

   ```bash
   cargo build --release -p cesarops-satellite --features gdal
   /data/cargo-target/release/sat-run /data/straits_optical_clear/2022/sentinel2_aws/B02_B03_B04_B08_Cedarville.json
   ```

2. After the Cedarville calibration is complete, run the `straits_local_run.json` mission using the following command:

   ```bash
   /data/cargo-target/release/sat-run /data/straits_optical_clear/2022/sentinel2_aws/straits_local_run.json
   ```

Remember to perform the Cedarville calibration run before executing the `straits_local_run.json` mission.

Acceptance Checklist:

1. Optical candidate flagged within 300m of BAG masked wreck at 45.87127°N, -84.58642°W.
2. Cedarville steel (45.7873°N, -84.6708°W) passes calibration test.
3. Eber Ward, William Young, M. Stalker, and Elva controls confirmed.
4. B02 blue + B03 green (column) and B04/B08 surface bands used.
5. Wood vs steel discrimination using thermal, zebra-clarity, and glint.
6. Data sourced from data/straits\_optical\_clear|2022|2023/sentinel2\_aws.
7. Processing executed on 32-core CPU with rayon, or with 2× P100 via cudarc if necessary.
8. cesarops-satellite on T440 path remains unedited.

Expected Report Artifacts:

1. A map showing the 300m rule around the BAG masked wreck.
2. Calibration results for the Cedarville steel, confirming it passes the test.
3. Confirmation of the controls (Eber Ward, William Young, M. Stalker, and Elva).
4. Visualizations of the B02 blue + B03 green (column) and B04/B08 surface bands.
5. Results of the wood vs steel discrimination, highlighting the chosen candidate.
6. A summary of the data used, including source and processing details.
7. Performance metrics for the CPU and GPU processing, if applicable.
8. A statement confirming that cesarops-satellite on T440 path remains unedited.