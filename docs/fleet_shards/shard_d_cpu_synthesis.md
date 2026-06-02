## GPU optional plan (go/no-go criteria)

**GO** criteria:

- If the calibration using Cedarville steel at 45.7873°N, -84.6708W is not met with the desired accuracy using rayon on 32 Xeon cores, proceed with GPU processing.
- If the CPU throughput is insufficient to process the data from `data/straits_optical_clear|2

The cross-sensor confirmation story involves using both bathymetric data from the BAG system and optical data from Sentinel-2 satellites to detect and confirm underwater wreckage. The BAG system provides high-resolution bathymetric relief data, while Sentinel-2 satellites capture optical images of the water column.

The process begins with the BAG system detecting a hard target, indicating potential wreckage on the lake floor. A 300-meter tolerance zone is established around the detected target to account for potential errors in the BAG data. Within this zone, Sentinel-2 satellites look for anomalies in the water column, such as a plume or discoloration, which could indicate the presence of wreckage.

The Sentinel-2 data is captured in specific bands, with B02 blue and B03 green used to detect the water column anomaly, while B04 and B08 are used to identify surface features. The depth of the water column is also taken into account, as it affects the amount of bottom reflectance in the Sentinel-2 data.

To confirm the presence of wreckage, the optical signal from Sentinel-2 is compared to the bathymetric relief data from the BAG system. If the two datasets correspond, it suggests the presence of a wreckage site. This cross-sensor confirmation approach is used to increase the accuracy and reliability of underwater wreckage detection.

In summary, the cross-sensor confirmation story involves using both bathymetric relief data from the BAG system and optical data from Sentinel-2 satellites to detect and confirm underwater wreckage. By comparing the two datasets, researchers can increase the accuracy and reliability of their detection methods.

* Objective: Flag optical candidate near a specific BAG masked wreck location (45.87127°N, -84.58642°W) using Sentinel-2 data, supporting the Robert Burns hypothesis of a water-column plume signal.
* Calibration gate: Validate the pipeline using a known steel wreck (Cedarville) at 45.7873°N, -84.6708W before applying it to the target wreck.
* Physics: Utilize bands B02 blue and B03 green for water column plume detection, while considering thermal bands B04/B08 for surface differentiation between wood and steel.
* Execution path: Process data from data/straits\_optical\_clear|2022|2023/sentinel2\_aws using rayon on 32 cores, with optional support from 2× P100 GPUs via cudarc if CPU throughput is insufficient.
* Success criterion: Accurate identification and flagging of potential optical candidates within the specified distance from the target wreck location, demonstrating the feasibility of the proposed method.