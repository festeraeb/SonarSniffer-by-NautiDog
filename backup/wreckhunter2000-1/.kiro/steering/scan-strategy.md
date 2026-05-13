# CESAROPS Scan Strategy — Multi-Day Tile Stacking & Weather-Driven Acquisition

This document defines the core scanning philosophy. Every agent, worker, and pipeline
in this system MUST follow these principles when selecting satellite imagery dates
or planning scan passes.

## Core Principle: Stack 20+ Days of Tiles

A single satellite pass is NEVER sufficient to confirm a wreck. You need temporal stacking:
- **Minimum 20 days** of imagery over the same tile
- Different weather conditions reveal different features
- The combination of calm + post-storm + thermal contrast days gives the full picture

## Weather Window Categories

### 1. CALM DAYS (optical baseline + SWOT/ICESat-2 surface height)
- Wind < 5 mph, no precipitation
- Best for: optical clarity, shallow-water bathymetry, sun glint analysis
- Use for: baseline seafloor features, hull outline detection in clear water
- Season: July–October (Great Lakes)

### 2. POST-STORM DAYS (plume detection, 1-3 days after storm)
- Storm surge stirs sediment off lakebed structures (wrecks act as flow obstacles)
- **Day 1 post-storm**: strongest plume/surge signal — sediment cloud visible in optical
- **Day 2 post-storm**: plume dispersing, displacement patterns visible in SAR
- **Day 3 post-storm**: tail of plume, sediment settling, thermal anomaly from mixing
- Key: N/NW winds ≥ 20 mph for 24+ hours, then clearing
- The wreck hull disrupts the storm surge flow → visible wake/plume downstream

### 3. LOW WATER DAYS (exposure events)
- Great Lakes water levels fluctuate seasonally and with wind setup/setdown
- Strong sustained offshore wind → water level drops 1-3 feet locally (seiche)
- Wrecks in 10-20ft depth may become partially exposed or create visible shoaling
- Check NOAA water level gauges for negative departures > 1 foot

### 4. THERMAL CONTRAST DAYS (heat sink detection)
- Steel hulls absorb/release heat differently than surrounding lakebed
- **Hot sunny days after cold nights**: wreck heats faster → thermal IR anomaly
- **Cold snaps after warm period**: wreck retains heat longer → warm spot in thermal
- Best detected with Landsat 8/9 Band 10 (thermal IR) or ECOSTRESS
- Requires clear sky (no cloud cover in thermal bands)

### 5. SAR TEXTURE DAYS (roughness contrast)
- SAR (Sentinel-1) penetrates clouds — use anytime
- Calm water + submerged wreck = texture anomaly (wreck disrupts wave patterns)
- Post-storm SAR shows sediment plume boundaries as backscatter changes
- Ice-free season only (ice confounds SAR texture analysis)

## Seiche & Surge Awareness

Great Lakes seiches are standing waves caused by wind setup:
- Lake Erie: most susceptible (shallow, elongated E-W)
- A strong W/SW wind pushes water to the east end → west end drops 3-6 feet
- When wind stops, water sloshes back (seiche period ~14 hours for Erie)
- **Scan the UPWIND end during sustained wind** for low-water exposure
- **Scan the DOWNWIND end after wind stops** for surge-driven plume events

## Runoff & Tributary Plumes

After heavy rain:
- River mouths produce sediment plumes that can mask or reveal features
- Turbidity increases → optical useless near tributaries
- BUT: the plume boundary itself can reveal submerged obstacles (flow deflection)
- Wait 2-3 days after rain for plume to clear from target area

## Implementation Rules for Worker Agents

1. **NEVER download a single date** — always request the full temporal stack
2. **Tag each tile with its weather condition** (calm/post_storm_N/thermal/sar_texture)
3. **Weight the stack**: post-storm day 1 gets 3× weight, calm gets 1×, transitional gets 0.5×
4. **Reject tiles with > 30% cloud cover** for optical (SAR is always usable)
5. **Cross-reference NOAA buoy data** for the exact tile acquisition time
6. **Log which weather window each detection came from** — this trains the ML model

## ML Training Implications

The machine learning models MUST be trained on weather-tagged data:
- Feature: `weather_condition` (calm/post_storm_1/post_storm_2/thermal_contrast/sar_texture)
- Feature: `days_since_storm` (0 = during storm, 1-3 = post-storm, >3 = baseline)
- Feature: `water_level_departure_ft` (from NOAA gauge, negative = low water)
- Feature: `thermal_delta_c` (air temp change in last 24h — proxy for thermal contrast)
- Feature: `wind_speed_at_acquisition` (from nearest buoy, at satellite overpass time)

Models trained without weather context will have high false-positive rates because
they can't distinguish "wreck plume" from "natural turbidity" or "wreck thermal" from
"shallow-water warming".
