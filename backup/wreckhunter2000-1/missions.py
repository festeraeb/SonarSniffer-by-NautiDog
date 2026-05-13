#!/usr/bin/env python3
"""
CESAROPS Mission Registry
=========================
Single source of truth for ALL scan missions.

To add a new mission: add one dict to MISSIONS below and call:
    python run_mission.py <mission_name>
    python run_mission.py <mission_name> --push-queue

Nothing else to build. The scan engine handles the rest.
"""

from datetime import date, timedelta

def _rolling(days: int) -> tuple[str, str]:
    """Return (start, end) for the most recent N days."""
    end   = date.today()
    start = end - timedelta(days=days)
    return start.strftime("%Y-%m-%d"), end.strftime("%Y-%m-%d")


# ── CMR concept IDs ───────────────────────────────────────────────────────────
CMR = {
    "S1_GRD":    "C1214470488-ASF",    # Sentinel-1 GRD Level-1  (ASF global)
    "S1_RTC":    "C2036882064-ASF",    # Sentinel-1 RTC          (ASF fallback)
    "HLS_S30":   "C2021957295-LPCLOUD",# HLS Sentinel-2 30m
    "HLS_L30":   "C2021957657-LPCLOUD",# HLS Landsat 30m
}


# ── Pass definitions ──────────────────────────────────────────────────────────
# Each pass key maps to a function in scan_engine.py.
# "thresholds" overrides defaults per mission.

PASS_DEFAULTS = {
    "sar_bright":    {"z_thresh": 3.0, "window": 20},
    "sar_change":    {"z_thresh": 2.5},
    "dark_vessel":   {"radius_m": 500},
    "turbidity":     {"z_thresh": 2.0},
    "optical_surf":  {"reflectance_thresh": 0.15},
    "stumpf":        {"z_thresh": 1.8},
    "thermal_cold":  {"z_thresh": 1.5, "invert": True},
    "nir_anomaly":   {"z_thresh": 2.0},
    "swir_fuel":     {"z_thresh": 2.2},
    "ice_fracture":  {"z_thresh": 2.5},  # SAR ice impact fracture
}


# ── Risk zone helpers ─────────────────────────────────────────────────────────

def _risk_zone(label, lat, lon, risk, passes, note):
    return {"label": label, "lat": lat, "lon": lon,
            "risk": risk, "passes": passes, "note": note}


# =============================================================================
# MISSIONS
# =============================================================================

MISSIONS = {

    # ── Strait of Hormuz — maritime mine hazard ───────────────────────────────
    "hormuz_mines": {
        "label":        "Strait of Hormuz — Mine Hazard",
        "target_type":  "mine_hazard",
        "output_dir":   "hormuz",

        # Rolling 30 days — always pull newest available imagery
        "date_range":   lambda: _rolling(30),

        "bbox":         [25.0, 54.0, 27.5, 58.5],   # [lat_min, lon_min, lat_max, lon_max]
        "sub_zones": {
            "inbound_tss":   [25.8, 56.2, 26.3, 56.8],
            "outbound_tss":  [26.1, 56.4, 26.7, 57.1],
            "bottleneck":    [26.1, 56.3, 26.8, 57.0],
            "abu_musa":      [25.7, 55.0, 25.95, 55.2],
            "qeshm_north":   [26.6, 55.6, 27.2, 56.7],
            "khasab_bay":    [26.1, 56.2, 26.5, 56.7],
        },
        "risk_lanes":   ["inbound_tss", "outbound_tss", "bottleneck"],  # elevates risk score
        "shallow_zones": [
            [25.7, 55.0, 25.95, 55.2],   # Abu Musa shelf (<50m)
            [25.5, 55.4, 26.0, 56.2],    # Tunb islands shelf
        ],

        "sensors": {
            "sar":     CMR["S1_GRD"],
            "sar_rtc": CMR["S1_RTC"],    # fallback
            "optical": CMR["HLS_S30"],
        },
        "passes":       ["sar_bright", "sar_change", "dark_vessel",
                          "turbidity", "optical_surf"],
        "thresholds": {
            "max_cloud":    60.0,   # Middle East dust/haze — need high tolerance
            "sar_bright":   {"z_thresh": 3.0},
            "sar_change":   {"z_thresh": 2.5},
            "turbidity":    {"z_thresh": 2.0},
        },

        "risk_scoring": {
            # (n_passes_fired, in_risk_lane) → risk level
            # CRITICAL: 3+ passes or 2+ in transit lane
            # HIGH: 2 passes or 1 sar_change in TSS
            "CRITICAL": lambda n, tss, passes: n >= 3 or (tss and n >= 2),
            "HIGH":     lambda n, tss, passes: n == 2 or (tss and "sar_change" in passes),
            "MEDIUM":   lambda n, tss, passes: n == 1 and tss,
            "LOW":      lambda n, tss, passes: True,  # fallback
        },

        "historical_zones": [
            _risk_zone("Abu Musa Island approaches", 25.877, 55.033, "HIGH",
                       ["historical", "known_incident"],
                       "IRGC mined approaches 1987–88 (Tanker War). Shallow shelf <30m. Active IRGC naval base."),
            _risk_zone("Greater Tunb Island — W approach", 26.253, 55.280, "HIGH",
                       ["historical", "transit_lane_adjacent"],
                       "Iran-occupied. Shallow reef shelf. Approaches cross inbound TSS within 12nm."),
            _risk_zone("TSS Bottleneck — Sir Bu Nu'Ayr", 25.228, 54.250, "MEDIUM",
                       ["transit_lane", "chokepoint"],
                       "Narrowest chokepoint. USN/NAVCENT Notices to Mariners mine susceptibility zone."),
            _risk_zone("Lesser Tunb Island", 26.238, 55.143, "MEDIUM",
                       ["historical"],
                       "Uninhabited Iran-controlled island. Shelf < 40m depth."),
            _risk_zone("Qeshm North Channel", 26.801, 56.044, "MEDIUM",
                       ["chokepoint", "shallow_shelf"],
                       "Narrow channel north of Qeshm. Depths 15–40m. Vessel AIS gap zone."),
            _risk_zone("Outbound TSS — eastern exit", 26.401, 57.020, "LOW",
                       ["transit_lane"],
                       "Eastern exit of outbound lane. Depths >60m, less mine-suitable."),
            _risk_zone("Khasab Bay approaches", 26.197, 56.257, "LOW",
                       ["ais_gap_zone"],
                       "Oman. High small-boat traffic; AIS coverage poor. False positive risk zone."),
        ],

        "queue_jobs": [
            {"label": "hormuz_full_sar",  "bbox_key": "bbox",         "sensors": ["sar", "optical"]},
            {"label": "hormuz_bottleneck","bbox_key": "bottleneck",   "sensors": ["sar"]},
            {"label": "hormuz_abu_musa",  "bbox_key": "abu_musa",     "sensors": ["sar", "optical"]},
        ],

        "notes": (
            "IMPORTANT: Civilian maritime safety research tool only. "
            "All flagged areas require assessment by qualified maritime authorities "
            "before any navigation advisory is issued."
        ),
    },

    # ── Northwest Airlines Flight 2501 — Lake Michigan ────────────────────────
    "nwa2501": {
        "label":        "NWA Flight 2501 — Lake Michigan",
        "target_type":  "submerged_aircraft",
        "output_dir":   "nwa2501",

        "date_range":   ("2020-06-01", "2024-08-31"),  # clear summer window

        "bbox":         [43.05, -86.85, 43.35, -86.45],   # primary
        "sub_zones": {
            "primary":  [43.05, -86.85, 43.35, -86.45],
            "extended": [42.85, -87.00, 43.50, -86.30],
        },
        "last_known":   (43.12, -86.65),
        "depth_range":  (30, 120),   # metres

        "sensors": {
            "optical": CMR["HLS_S30"],
            "hls_l30": CMR["HLS_L30"],
            "sar":     "C1214354438-ASF",   # S1 GRD legacy ASF concept for Great Lakes
        },
        "passes":       ["stumpf", "thermal_cold", "nir_anomaly", "swir_fuel", "sar_bright"],
        "thresholds": {
            "max_cloud":    15.0,   # Lake Michigan — want clear sky only
            "stumpf":       {"z_thresh": 1.8},
            "thermal_cold": {"z_thresh": 1.5},
            "nir_anomaly":  {"z_thresh": 2.0},
            "swir_fuel":    {"z_thresh": 2.2},
            "sar_bright":   {"z_thresh": 3.0},
        },
        "preferred_months": [6, 7, 8],   # peak Secchi depth

        "historical_zones": [
            _risk_zone("NWA 2501 Last Known Position", 43.12, -86.65, "HIGH",
                       ["last_known"],
                       "Dead reckoning from Chicago VOR, 23:51 EST 1950-06-23. "
                       "Debris washed ashore near South Haven MI June 24."),
        ],

        "queue_jobs": [
            {"label": "NWA_2501_primary",  "bbox_key": "primary",  "sensors": ["optical", "sar"]},
            {"label": "NWA_2501_extended", "bbox_key": "extended", "sensors": ["optical"]},
        ],
    },

    # ── Alaska Canopy (spruce beetle / boreal defoliation) ────────────────────
    "alaska_canopy": {
        "label":       "Alaska Boreal Canopy — Spruce Beetle Assessment",
        "target_type": "canopy_health",
        "output_dir":  "alaska_canopy",

        "date_range":  ("2023-06-01", "2025-09-30"),

        "bbox":        [63.5, -153.0, 65.5, -145.0],
        "sub_zones": {
            "areaA": [63.5, -153.0, 64.5, -150.0],
            "areaB": [64.0, -151.0, 65.0, -148.0],
            "areaC": [64.5, -150.0, 65.5, -145.0],
        },

        "sensors": {
            "optical": CMR["HLS_S30"],
        },
        "passes":      ["nir_anomaly", "swir_fuel"],   # swir_fuel reused as SWIR stress index
        "thresholds": {
            "max_cloud":   20.0,
            "nir_anomaly": {"z_thresh": 2.0},
            "swir_fuel":   {"z_thresh": 1.8},
        },
        "preferred_months": [7, 8],

        "queue_jobs": [
            {"label": "alaska_canopy_areaA", "bbox_key": "areaA", "sensors": ["optical"]},
            {"label": "alaska_canopy_areaB", "bbox_key": "areaB", "sensors": ["optical"]},
            {"label": "alaska_canopy_areaC", "bbox_key": "areaC", "sensors": ["optical"]},
        ],
    },

    # ── Nome Alaska — Cessna Grand Caravan 208B (Feb 8-9 2025) ───────────────
    "nome_cessna": {
        "label":        "Nome AK — Cessna 208B Grand Caravan (Feb 2025)",
        "target_type":  "surface_aircraft",   # on/through sea ice
        "output_dir":   "nome_cessna",

        # Specific incident dates — SAR has 6-day repeat so bracket by ±10 days
        # to catch the first pass after impact
        "date_range":   ("2025-01-28", "2025-02-20"),

        # 35nm radius from Nome (64.499°N, 165.410°W)
        # 35nm ≈ 64.8km → ~0.58° lat, ~1.4° lon at 64.5°N
        "bbox":         [63.8, -167.5, 65.2, -163.5],
        "sub_zones": {
            "primary_35nm":   [64.0,  -167.0, 65.0, -163.8],  # 35nm off Nome
            "shore_zone":     [64.3,  -165.8, 64.7, -164.8],  # <10nm, possible debris drift
            "norton_sound":   [63.8,  -165.0, 64.5, -162.5],  # SE drift zone
            "bering_shelf":   [63.8,  -167.5, 64.5, -165.5],  # Western approach
        },
        "last_known":   (64.50, -165.41),   # Nome airport vicinity / coastline

        "sensors": {
            "sar": CMR["S1_GRD"],     # Primary — SAR sees through ice cloud cover
        },
        "passes":       ["sar_bright", "ice_fracture", "sar_change"],
        "thresholds": {
            "max_cloud":       100.0,  # Feb Arctic — optical useless, SAR only
            "sar_bright":      {"z_thresh": 2.8, "window": 15},
                                       # Lower z-thresh: aircraft wreckage on ice
                                       # is a discrete high-backscatter point in
                                       # otherwise uniform sea-ice background
            "ice_fracture":    {"z_thresh": 2.5},
                                       # Impact fracture in sea ice = linear low-
                                       # backscatter streak from impact direction
            "sar_change":      {"z_thresh": 2.0},
                                       # Compare pre-impact (Jan 28) vs post-impact
                                       # (Feb 14+) pass: new return = debris field
        },

        "historical_zones": [
            _risk_zone("Nome coastline — 35nm bearing", 64.50, -165.97, "HIGH",
                       ["last_known", "sar_bright"],
                       "Feb 8-9 2025. Cessna 208B, ~35nm off Nome coast. "
                       "Norton Sound fully ice-covered in February. "
                       "Aircraft likely impacted on/through sea ice pack. "
                       "Metal airframe = high VV SAR return against uniform ice background. "
                       "Fuselage: 12.7m long, 3.5m span — resolvable at Sentinel-1 10m GRD."),
            _risk_zone("Norton Sound ice edge zone", 64.15, -165.50, "MEDIUM",
                       ["ice_fracture", "drift_zone"],
                       "Sea ice drift in February Norton Sound moves ~0.3-0.8 km/day "
                       "from NW to SE. Debris field may have moved 3-7km from impact "
                       "site by first available SAR pass."),
            _risk_zone("Bering shelf shoal area", 64.05, -166.80, "LOW",
                       ["sar_change"],
                       "Shallow shelving ice — possible secondary debris concentration "
                       "zone if aircraft broke up before impact."),
        ],

        "queue_jobs": [
            {"label": "nome_cessna_primary",   "bbox_key": "primary_35nm",  "sensors": ["sar"]},
            {"label": "nome_cessna_shore",      "bbox_key": "shore_zone",    "sensors": ["sar"]},
            {"label": "nome_cessna_norton",     "bbox_key": "norton_sound",  "sensors": ["sar"]},
        ],

        "notes": (
            "February Norton Sound / Bering Sea is fully ice-covered. "
            "Optical imagery not useful — SAR only. "
            "Key signature: high-VV backscatter point target on uniform sea-ice background. "
            "Secondary: linear fracture streak in ice from impact direction. "
            "Ice drift correction required if >3 days post-impact."
        ),
    },

}


# ── Convenience getters ───────────────────────────────────────────────────────

def get(mission_name: str) -> dict:
    m = MISSIONS.get(mission_name)
    if m is None:
        raise KeyError(f"Unknown mission '{mission_name}'. "
                       f"Available: {list(MISSIONS.keys())}")
    # Resolve rolling date if callable
    m = dict(m)
    if callable(m.get("date_range")):
        m["date_range"] = m["date_range"]()
    return m


def list_missions() -> None:
    print("Available missions:")
    for name, m in MISSIONS.items():
        dr = m["date_range"]
        if callable(dr):
            dr = "<rolling>"
        print(f"  {name:<22}  {m['label']}")
        print(f"  {'':22}  bbox={m['bbox']}  dates={dr}")
        print()


if __name__ == "__main__":
    list_missions()
