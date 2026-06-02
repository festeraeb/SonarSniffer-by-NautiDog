"""Great Lakes historic newspaper harvest configuration."""

from __future__ import annotations

# Chronicling America via loc.gov (2025+ API)
LOC_COLLECTION = "https://www.loc.gov/collections/chronicling-america/"

GL_STATES = [
    "michigan",
    "wisconsin",
    "ohio",
    "illinois",
    "indiana",
    "new york",
    "pennsylvania",
    "minnesota",
]

# Prefer these newspaper titles (partof_title) per state
STATE_NEWSPAPERS: dict[str, list[str]] = {
    "michigan": [
        "Detroit Free Press",
        "Detroit Evening Times",
        "Detroit Tribune",
        "Grand Rapids Press",
        "Bay City Tribune",
        "Marquette Mining Journal",
    ],
    "wisconsin": ["Milwaukee Sentinel", "Green Bay Gazette"],
    "ohio": ["Cleveland Plain Dealer", "Sandusky Register"],
    "illinois": ["Chicago Tribune", "Chicago Daily News"],
    "new york": ["Buffalo Express", "Buffalo Courier"],
    "pennsylvania": ["Erie Dispatch"],
    "minnesota": ["Duluth Herald"],
    "indiana": ["Gary Tribune"],
}

# Text must match at least one to count as Great Lakes–relevant
GL_RELEVANCE_PATTERNS = [
    "great lakes",
    "lake superior",
    "lake michigan",
    "lake huron",
    "lake erie",
    "lake ontario",
    "sault",
    "soo canal",
    "straits of mackinac",
    "detroit river",
    "thousand islands",
    "niagara",
    "door county",
    "whitefish point",
    "thunder bay",
    "keweenaw",
]

# Major Great Lakes port cities (for partof_title / qs boosting)
GL_PORTS = [
    "Detroit",
    "Chicago",
    "Cleveland",
    "Buffalo",
    "Milwaukee",
    "Port Huron",
    "Sandusky",
    "Erie",
    "Duluth",
    "Marquette",
    "Sault Ste Marie",
    "Sault Sainte Marie",
    "Traverse City",
    "Green Bay",
    "Toledo",
    "Racine",
    "Muskegon",
    "Grand Haven",
    "Saginaw",
    "Bay City",
    "Alpena",
    "Cheboygan",
    "Manitowoc",
    "Ashtabula",
    "Conneaut",
]

# 19th-century wreck / storm terminology
WRECK_KEYWORDS = [
    '"lost with all hands"',
    '"white hurricane"',
    "gale",
    "schooner",
    "steamer",
    "propeller",
    '"missing vessel"',
    "overdue",
    "capsized",
    "foundered",
    "went down",
    "total loss",
    "marine disaster",
    "shipwreck",
    "struck reef",
    "foundered on",
]

# Retrospective / early-wreck compilations (1600s–1700s via 1800s press)
RETROSPECTIVE_QUERIES = [
    '"disasters on the lakes"',
    '"disasters on the lakes since"',
    '"early disasters" great lakes',
    "Griffon wreck",
    "HMS Ontario",
    '"marine disasters" lakes history',
    '"list of wrecks" lakes',
]

# Supplementary source URLs (manual / future scrapers)
SUPPLEMENTARY_SOURCES = {
    "michigan_digital_newspapers": "https://digmichnews.cmich.edu/",
    "maritime_history_great_lakes": "https://www.maritimehistoryofthegreatlakes.ca/",
}

DEFAULT_START_DATE = "1836-01-01"
DEFAULT_END_DATE = "1924-12-31"

REQUEST_DELAY_SEC = 1.0
MAX_PAGES_PER_QUERY = 5  # 500 items max per query @ c=100
