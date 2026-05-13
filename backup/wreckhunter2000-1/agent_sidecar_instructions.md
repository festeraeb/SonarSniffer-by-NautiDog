# CESAROPS Agent Sidecar Instructions

These instructions represent the tool definitions (schemas) and operational system prompts for your mastermind agent (e.g., n8n agent node, Microsoft Foundry agent, or LangChain wrapper) to correctly invoke the appropriate Dockerized sidecar specialist for each specific task.

The local Docker Swarm / Compose uses `cesarops-specialist` containers, dynamically assigned a role port.

---

## 1. `satellite_water` (Port 8001)

### **Goal:**
Locate deep-water marine targets like wrecks and cold-sinks using SAR, Optical, and Thermal.
Matches target characteristics: "Steel, Submerged, Deep Water, Shipping Lane, Thermal Sink".

### **Tool Schema (OpenAPI / Function Calling):**
```json
{
  "name": "scan_satellite_water",
  "description": "Scans marine areas for submerged wrecks using thermal/optical/SAR combinations (The TRIPLE-LOCK method). Returns anomaly coordinate bounding boxes and confidence scores.",
  "parameters": {
    "type": "object",
    "properties": {
      "lat": {"type": "number", "description": "Center latitude of search grid"},
      "lon": {"type": "number", "description": "Center longitude of search grid"},
      "radius_km": {"type": "number", "description": "Radius in km to scan."},
      "depth_profile": {
        "type": "string",
        "enum": ["shallow", "deep_500ft"],
        "description": "Sets filtering thresholds. 'shallow' utilizes standard optics. 'deep_500ft' enforces Thermal/SAR-only due to light attenuation."
      }
    },
    "required": ["lat", "lon", "radius_km"]
  }
}
```

### **HTTP Mapping:**
`POST http://localhost:8001/api/v1/scan/satellite/water`

---

## 2. `satellite_land` (Port 8002)

### **Goal:**
Locate terrestrial crashes (aircraft, remote vehicles) utilizing NDVI shifts (chlorophyll displacement) and burn scars.
Matches target characteristics: "Forest, Land, Aircraft, Burn Scar, Disrupted Canopy".

### **Tool Schema (OpenAPI / Function Calling):**
```json
{
  "name": "scan_satellite_land",
  "description": "Scans terrestrial land areas to find disrupted canopies, burn scars, or aircraft crash sites utilizing NDVI vegetation indices.",
  "parameters": {
    "type": "object",
    "properties": {
      "lat": {"type": "number", "description": "Center latitude of search grid"},
      "lon": {"type": "number", "description": "Center longitude of search grid"},
      "radius_km": {"type": "number", "description": "Radius in km to scan."},
      "date_start": {"type": "string", "description": "YYYY-MM-DD to constrain search roughly 1 week before incident."},
      "date_end": {"type": "string", "description": "YYYY-MM-DD constraint (e.g. 2 weeks after constraint to find new scars)."}
    },
    "required": ["lat", "lon", "radius_km"]
  }
}
```

### **HTTP Mapping:**
`POST http://localhost:8002/api/v1/scan/satellite/land`

---

## 3. `magnetics` (Port 8003)

### **Goal:**
Locate historical aerial magnetic survey anomalies typically revealing massive buried steel (freighters or ore-carriers) beneath silt/sand that visual arrays cannot penetrate.

### **Tool Schema (OpenAPI / Function Calling):**
```json
{
  "name": "scan_magnetics",
  "description": "Queries historical USGS/aeromagnetic survey databases to find magnetic deviation clusters. Use when visual observation is hampered by extreme silt or burial.",
  "parameters": {
    "type": "object",
    "properties": {
      "lat": {"type": "number", "description": "Center latitude of search grid"},
      "lon": {"type": "number", "description": "Center longitude of search grid"},
      "radius_km": {"type": "number", "description": "Radius in km to filter the DB scan."}
    },
    "required": ["lat", "lon", "radius_km"]
  }
}
```

### **HTTP Mapping:**
`POST http://localhost:8003/api/v1/scan/mag`

---

## 4. `sonar` (Port 8004)

### **Goal:**
Perform high-resolution structural mapping of a target that has *already been located*. Do not use this for wild searches. Uses NOAA BAG Files and point clouds.

### **Tool Schema (OpenAPI / Function Calling):**
```json
{
  "name": "scan_sonar_bag",
  "description": "Performs geometric bounding, dimensional analysis, and depth contour mapping using NOAA BAG sonar files. Use this ONLY when a confirmed target BAG tile is known.",
  "parameters": {
    "type": "object",
    "properties": {
      "filepath": {
        "type": "string",
        "description": "The exact absolute path or relative mapped volume path to the .bag file (e.g., '/app/downloads/H12345.bag')."
      }
    },
    "required": ["filepath"]
  }
}
```

### **HTTP Mapping:**
`POST http://localhost:8004/api/v1/scan/bag`

---

## 🧠 Agent Logic Priority (Instructions for the Orchestrating LLM):

1. **Evaluate Target Substrate:** If the prompt specifies a "water" target, NEVER call `scan_satellite_land`. If it's a land target (aircraft), NEVER call `scan_satellite_water`.
2. **Handle Vague/Amnesiac Queries:** If the user provides a grid with no context, explicitly ask: "Is this target Marine (lake/ocean) or Terrestrial (land)?"
3. **Execute The Triple-Lock:** If relying solely on optics (`satellite_water`), be aware noise is common. To reinforce confidence, pass the latitude/longitude hits from `scan_satellite_water` into `scan_magnetics` to see if there is an overlapping magnetic signature. 
4. **Pre-requisite Validation:** Do NOT invoke `scan_sonar_bag` without a `filepath`. It cannot perform a lat/lon search. Use standard satellite tools for wild wide-area searches first. 
