"""Great Lakes shipping lane reference for LLM wreck-location reasoning."""

from __future__ import annotations

# Named corridors (approximate mid-line points and bearing ° from north)
GL_SHIPPING_LANES = [
    {
        "name": "Lake Superior — Duluth to Sault Ste. Marie",
        "bearing_deg": 90,
        "lat": 46.8,
        "lon": -87.5,
        "notes": "Ore carriers; Whitefish Bay approaches",
    },
    {
        "name": "Lake Michigan — Chicago to Mackinac",
        "bearing_deg": 15,
        "lat": 44.5,
        "lon": -87.0,
        "notes": "Main Chicago–Michigan passenger and package trade",
    },
    {
        "name": "Lake Michigan — Milwaukee to Grand Haven/Muskegon",
        "bearing_deg": 350,
        "lat": 43.5,
        "lon": -87.2,
        "notes": "West-shore lumber and package routes",
    },
    {
        "name": "Lake Huron — Port Huron to Straits",
        "bearing_deg": 75,
        "lat": 43.5,
        "lon": -82.5,
        "notes": "St. Clair River / lower Huron traffic",
    },
    {
        "name": "Lake Huron — Straits of Mackinac crossing",
        "bearing_deg": 60,
        "lat": 45.8,
        "lon": -84.7,
        "notes": "Heavy traffic; many historic losses",
    },
    {
        "name": "Lake Erie — Detroit River to Buffalo",
        "bearing_deg": 75,
        "lat": 42.2,
        "lon": -81.5,
        "notes": "Cleveland–Buffalo corridor ~075°/255°",
    },
    {
        "name": "Lake Erie — Toledo/Sandusky to Cleveland",
        "bearing_deg": 70,
        "lat": 41.6,
        "lon": -82.3,
        "notes": "Southern shore bulk and passenger",
    },
    {
        "name": "Lake Ontario — Niagara to Kingston corridor",
        "bearing_deg": 85,
        "lat": 43.5,
        "lon": -77.8,
        "notes": "St. Lawrence approach traffic",
    },
]


def lanes_context_block() -> str:
    lines = ["Great Lakes shipping lanes (use for last-seen / loss corridor reasoning):"]
    for lane in GL_SHIPPING_LANES:
        lines.append(
            f"- {lane['name']}: bearing ~{lane['bearing_deg']}°, "
            f"ref ({lane['lat']}, {lane['lon']}). {lane['notes']}"
        )
    return "\n".join(lines)


def nearest_lane_hint(lat: float, lon: float) -> str | None:
    """Rough nearest lane by distance to reference point."""
    import math

    best, best_d = None, 1e18
    for lane in GL_SHIPPING_LANES:
        d = (lat - lane["lat"]) ** 2 + (lon - lane["lon"]) ** 2
        if d < best_d:
            best_d, best = d, lane
    if best:
        return f"Nearest lane: {best['name']} ({best['notes']})"
    return None
