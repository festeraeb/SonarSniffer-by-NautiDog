"""Extract Great Lakes place hints from newspaper text and map to coordinates."""

from __future__ import annotations

import re
from dataclasses import dataclass

# Port / landmark gazetteer (approximate harbor coordinates)
GL_GAZETTEER: dict[str, tuple[float, float]] = {
    "detroit": (42.3314, -83.0458),
    "chicago": (41.8781, -87.6298),
    "cleveland": (41.4993, -81.6944),
    "buffalo": (42.8864, -78.8784),
    "milwaukee": (43.0389, -87.9065),
    "port huron": (42.9709, -82.4249),
    "sandusky": (41.4489, -82.7079),
    "erie": (42.1292, -80.0851),
    "duluth": (46.7867, -92.1005),
    "marquette": (46.5436, -87.3954),
    "sault ste marie": (46.4953, -84.3453),
    "sault sainte marie": (46.4953, -84.3453),
    "traverse city": (44.7631, -85.6206),
    "green bay": (44.5133, -88.0133),
    "toledo": (41.6528, -83.5379),
    "racine": (42.7261, -87.7829),
    "muskegon": (43.2342, -86.2484),
    "grand haven": (43.0631, -86.2284),
    "saginaw": (43.4195, -83.9508),
    "bay city": (43.5945, -83.8889),
    "alpena": (45.0617, -83.4328),
    "cheboygan": (45.6469, -84.4745),
    "manitowoc": (44.1008, -87.6576),
    "ashtabula": (41.8651, -80.7898),
    "conneaut": (41.9476, -80.5542),
    "whitefish point": (46.7707, -84.9481),
    "whitefish bay": (46.7707, -84.9481),
    "thunder bay": (45.0608, -83.4287),
    "munising": (46.4111, -86.6479),
    "copper harbor": (47.4685, -87.8865),
    "eagle harbor": (47.4557, -88.1593),
    "manitou island": (47.4167, -87.5833),
    "beaver island": (45.7225, -85.5362),
    "mackinac": (45.8492, -84.6189),
    "mackinac island": (45.8492, -84.6189),
    "straits of mackinac": (45.8, -84.7),
    "detour passage": (45.99, -83.9),
    "point aux barques": (44.0231, -82.9574),
    "sturgeon bay": (44.8342, -87.3770),
    "door county": (44.8342, -87.3770),
    "apostle islands": (46.9, -90.7),
    "isle royale": (48.0, -88.9),
    "keweenaw": (47.25, -88.45),
    "huron islands": (46.95, -87.95),
    "presque isle": (45.3453, -83.4972),
    "sandy hook": (40.4669, -74.0094),  # rarely GL — low priority
}

LAKE_REGIONS: list[tuple[re.Pattern[str], tuple[float, float], str]] = [
    (re.compile(r"lake\s+superior", re.I), (47.5, -87.0), "Lake Superior"),
    (re.compile(r"lake\s+michigan", re.I), (43.8, -87.0), "Lake Michigan"),
    (re.compile(r"lake\s+huron", re.I), (44.5, -82.5), "Lake Huron"),
    (re.compile(r"lake\s+erie", re.I), (42.2, -81.2), "Lake Erie"),
    (re.compile(r"lake\s+ontario", re.I), (43.8, -77.5), "Lake Ontario"),
    (re.compile(r"\bsuperior\b", re.I), (47.5, -87.0), "Lake Superior"),
    (re.compile(r"\bhuron\b", re.I), (44.5, -82.5), "Lake Huron"),
]

OFF_PATTERN = re.compile(
    r"\b(?:off|near|below|above|east of|west of|north of|south of)\s+"
    r"([A-Za-z][A-Za-z\s]{2,40}?)(?:\s|,|\.|$)",
    re.I,
)


@dataclass
class GeoHint:
    lat: float
    lon: float
    label: str
    confidence: float  # 0–1
    method: str


def _norm_place(s: str) -> str:
    return re.sub(r"\s+", " ", s.strip().lower())


def geocode_text(text: str) -> GeoHint | None:
    """Best single coordinate hint from article text."""
    if not text or len(text) < 20:
        return None
    t = text.lower()
    hints: list[GeoHint] = []

    for pat, (lat, lon), label in LAKE_REGIONS:
        if pat.search(text):
            hints.append(GeoHint(lat, lon, label, 0.35, "lake_region"))

    for name, (lat, lon) in GL_GAZETTEER.items():
        if name in t:
            conf = 0.75 if len(name) > 8 else 0.55
            hints.append(GeoHint(lat, lon, name.title(), conf, "gazetteer"))

    for m in OFF_PATTERN.finditer(text):
        place = _norm_place(m.group(1))
        if place in GL_GAZETTEER:
            lat, lon = GL_GAZETTEER[place]
            hints.append(GeoHint(lat, lon, f"off {place.title()}", 0.8, "off_phrase"))
        else:
            for gname, (lat, lon) in GL_GAZETTEER.items():
                if gname in place or place in gname:
                    hints.append(GeoHint(lat, lon, f"off {place.title()}", 0.65, "off_fuzzy"))
                    break

    if not hints:
        return None
    return max(hints, key=lambda h: h.confidence)


def geocode_historical_places(field: str | None) -> GeoHint | None:
    """Parse Swayze-style historical_place_names field."""
    if not field:
        return None
    return geocode_text(field)
