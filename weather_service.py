#!/usr/bin/env python3
"""
CESAROPS Weather Service — Open-Meteo API Integration

Provides historical and forecast weather data to optimize satellite scan planning.
Used to filter out cloudy dates (optical useless) and check ice cover (SAR penetration).

Open-Meteo API: https://open-meteo.com/ (No API Key Required)
"""

import requests
from datetime import datetime, timedelta
from typing import Dict, List, Optional

def get_historical_weather(lat: float, lon: float, start_date: str, end_date: str) -> List[Dict]:
    """
    Get historical weather data for the scan area.
    Returns daily averages for cloud cover, wind speed, and estimated ice cover.
    
    :param lat: Latitude
    :param lon: Longitude
    :param start_date: 'YYYY-MM-DD'
    :param end_date: 'YYYY-MM-DD'
    :return: List of daily weather dicts
    """
    url = "https://archive-api.open-meteo.com/v1/archive"
    params = {
        "latitude": lat,
        "longitude": lon,
        "start_date": start_date,
        "end_date": end_date,
        "daily": [
            "cloud_cover_mean",
            "wind_speed_10m_mean",
            "precipitation_sum",
        ],
        "timezone": "UTC",
    }
    
    try:
        resp = requests.get(url, params=params, timeout=10)
        resp.raise_for_status()
        data = resp.json().get("daily", {})
        
        results = []
        for i in range(len(data.get("time", []))):
            results.append({
                "date": data["time"][i],
                "cloud_cover": data.get("cloud_cover_mean", [None]*len(data["time"]))[i],
                "wind_speed": data.get("wind_speed_10m_mean", [None]*len(data["time"]))[i],
                "precipitation": data.get("precipitation_sum", [None]*len(data["time"]))[i],
            })
        return results
    except Exception as e:
        print(f"⚠️ Weather API failed: {e}")
        return []

def filter_good_optical_dates(weather_data: List[Dict], max_cloud_cover: float = 20.0) -> List[str]:
    """
    Filter dates with low cloud cover for optical scanning.
    
    :param weather_data: Output from get_historical_weather
    :param max_cloud_cover: Threshold in percent (0-100)
    :return: List of 'YYYY-MM-DD' strings
    """
    good_dates = []
    for day in weather_data:
        if day["cloud_cover"] is not None and day["cloud_cover"] < max_cloud_cover:
            good_dates.append(day["date"])
    return good_dates

def check_ice_risk(lat: float, lon: float, month: int) -> str:
    """
    Simple heuristic for Great Lakes ice cover based on latitude and month.
    (Open-Meteo doesn't provide direct ice cover, but this helps planning).
    
    :param lat: Latitude
    :param lon: Longitude
    :param month: 1-12
    :return: 'low', 'moderate', 'high'
    """
    # Northern Great Lakes freeze earlier and thicker
    is_north = lat > 45.0
    is_lake = True  # Assume lake for this bbox
    
    if is_north:
        if month in [1, 2, 3]: return "high"  # Feb/Mar peak ice
        if month in [12, 4]: return "moderate"
        return "low"
    else:
        if month in [1, 2]: return "moderate"
        if month == 3: return "low"
        return "low"


def classify_day_condition(
    weather_day: Dict,
    max_calm_wind: float = 15.0,
    min_storm_wind: float = 28.0,
    min_storm_precip: float = 3.0,
) -> str:
    """
    Classify a single weather day as 'calm', 'storm', or 'transitional'.

    Calm  : wind < max_calm_wind AND precip < 1 mm
             → good for optical wreck features, ICESat-2/SWOT surface height baseline
    Storm : wind >= min_storm_wind OR (wind >= 20 AND precip >= min_storm_precip)
             → SAR texture roughness elevated; post-storm plume planning window begins
    Transitional: everything else
    """
    wind   = float(weather_day.get("wind_speed")    or 0.0)
    precip = float(weather_day.get("precipitation") or 0.0)

    if wind >= min_storm_wind or (wind >= 20.0 and precip >= min_storm_precip):
        return "storm"
    elif wind <= max_calm_wind and precip < 1.0:
        return "calm"
    else:
        return "transitional"


def tag_storm_calm_pairs(
    weather_data: List[Dict],
    post_storm_days: int = 3,
) -> Dict[str, str]:
    """
    Classify each date in *weather_data* as one of:
        'calm'          — low wind, no precip; best optical baseline + SWOT/ICESat-2
        'storm'         — active storm; SAR texture, plume onset
        'post_storm_1'  — 1 day after storm ends; strongest plume/surge signal
        'post_storm_2'  — 2 days after; plume dispersing, displacement visible
        'post_storm_3'  — 3 days after; tail of plume, sediment settling
        'transitional'  — between calm and storm thresholds

    Post-storm passes over a wreck site catch sediment plumes stirred up by storm
    surge off the lakebed structure, and reveal water-column displacement as the
    storm surge flows over the wreck hull.

    :param weather_data: Output from get_historical_weather()
    :param post_storm_days: How many days after a storm to tag as post-storm (default 3)
    :return: Dict mapping 'YYYY-MM-DD' → condition string
    """
    from datetime import datetime as _dt, timedelta as _td

    # First pass: baseline classification
    conditions: Dict[str, str] = {}
    for day in weather_data:
        conditions[day["date"]] = classify_day_condition(day)

    # Second pass: propagate post-storm tags
    storm_dates = {d for d, c in conditions.items() if c == "storm"}
    for storm_date in sorted(storm_dates):
        dt = _dt.strptime(storm_date, "%Y-%m-%d")
        for offset in range(1, post_storm_days + 1):
            after = (dt + _td(days=offset)).strftime("%Y-%m-%d")
            if after in conditions and conditions[after] not in ("storm",):
                # Only override if not already a stronger post-storm label
                existing = conditions[after]
                existing_offset = (
                    int(existing.split("_")[-1])
                    if existing.startswith("post_storm_")
                    else 99
                )
                if offset < existing_offset:
                    conditions[after] = f"post_storm_{offset}"

    return conditions


def get_scan_windows(
    lat: float,
    lon: float,
    start_date: str,
    end_date: str,
    post_storm_days: int = 3,
) -> Dict[str, List[str]]:
    """
    Fetch weather for the date range and return grouped date lists ready for
    targeting satellite downloads.

    Returns a dict with keys:
        'calm'          → list of YYYY-MM-DD strings
        'storm'         → list of YYYY-MM-DD strings
        'post_storm_1'  → ...
        'post_storm_2'  → ...
        'post_storm_3'  → ...
        'transitional'  → ...
        'conditions'    → full Dict[str, str] mapping every date to its condition

    Example usage (central basin M&B2 search):
        windows = get_scan_windows(42.15, -81.25, '2023-01-01', '2024-12-31')
        calm_dates    = windows['calm']
        plume_dates   = windows['post_storm_1'] + windows['post_storm_2']
    """
    from collections import defaultdict as _dd
    weather = get_historical_weather(lat, lon, start_date, end_date)
    if not weather:
        return {"calm": [], "storm": [], "post_storm_1": [], "post_storm_2": [],
                "post_storm_3": [], "transitional": [], "conditions": {}}

    conditions = tag_storm_calm_pairs(weather, post_storm_days=post_storm_days)
    grouped: Dict[str, List[str]] = _dd(list)
    for date, cond in sorted(conditions.items()):
        grouped[cond].append(date)

    grouped["conditions"] = conditions  # type: ignore[assignment]
    return dict(grouped)
