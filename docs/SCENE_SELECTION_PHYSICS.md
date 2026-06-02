# Scene Selection Physics — operator's model (source of truth)

How the satellite pipeline chooses WHICH years, days, and conditions to image,
captured from the operator's field-proven reasoning. Implemented in
`cesarops-satellite/src/lake_levels.rs` (years/intent) and
`src/env_conditions.rs` (conditions/thermal regime).

## 1. Year selection is intent-driven, not "lowest water"

Low water is ONE strategy, not the default. `ScanIntent`:

| Intent | Best years | Why |
|--------|-----------|-----|
| `low_water_wreck` | lowest historic water level | wreck nearer the surface / readable zone |
| `recent_sinking` | most recent (incl. current year) | e.g. Charley Brown, Rosa — not there before |
| `zebra_clarity` | recent, AFTER the lows | mussel filtering clears the column over time; clearer later, not in the low year |
| `event_response` | most recent / event window | hydrocarbon spill, SAR — time-critical |
| `generic` | recent-first | no preference |

**Sensor constraint:** the Great Lakes record lows were 2012–2013. Sentinel-2
(2017+) CANNOT reach them — `lake_levels` clamps year selection to the sensor
archive and warns to use Landsat (1984+) for the true lows.

## 2. Thermal regime is set by DEPTH vs SUNLIGHT (not a day/night differential)

The key correction: day/night/season is about whether the wreck is **above or
below sunlight penetration**, not comparing the same wreck day-vs-night.

- **Deep wreck (below the photic zone, ~>200 ft):** never sees sunlight →
  **always cold**, day or night, all season. Persistent cold sink; its cold
  plume rising to the thermocline is readable any time. (The Andaste, 460+ ft.)
- **Shallow wreck (within the photic zone):** **heats all day** under sun,
  cools at night → cycles. Image at a thermal extreme (peak afternoon / pre-dawn).
- **Season shifts the cutoff:** the lit/warm layer is shallow in spring (~130 ft)
  and deep in summer (~220 ft), so a 150 ft wreck is "always cold" in April but
  "cycling" in August. `photic_depth_ft_for_month` encodes this.

`thermal_regime(depth_ft, month)` → `AlwaysCold | SunCycling`;
`preferred_pass` picks the acquisition strategy.

## 3. Plumes have two distinct sources — spring runoff is often the best

- **Spring runoff (freshet, Mar–May):** sediment plumes WITHOUT storm
  disturbance — cleaner signal, no wind-chop confounding the surface. Runoff
  also **raises current**, and higher current **raises the surface
  ripple/displacement over structure**, itself a surface-readable signal. For
  plume/spill intent this scores HIGHEST.
- **Post-storm (first calm day after a storm):** storm surge suspends lakebed
  sediment over the wreck; readable once the surface settles. Second-best plume
  window.
- **Clarity work penalises both** — runoff turbidity hurts the clear-water
  clarity read.

`classify_day` tags Calm / Storm / PostStorm(n) / SpringRunoff / Transitional
from Open-Meteo archive data; `condition_suitability(cond, intent)` returns the
0–1 weight the scene selector multiplies into cloud ranking.

## 4. The 20-day no-cloud stack

The operator's standard pull is a contiguous ~20-day low-cloud window
(`stack_window_days`, default 20) within each priority year, so the temporal
stack is tight enough that current-driven apparent-position drift can be
back-projected to the true seabed source.

## Triple-lock gold standard (gating, not selection)

A candidate is only called a wreck when **≥3 of 5 detectors** flag it
(human-in-the-loop standard). Selection above gets the right pixels in front of
the detectors; the triple-lock decides what counts. Plausibility checks (e.g.
"6 wrecks + a plane wing cannot all stack at 450–500 ft") remain a human gate,
a candidate for future automated encoding.
