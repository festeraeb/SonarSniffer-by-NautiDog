from fastapi import FastAPI, HTTPException
from pydantic import BaseModel
import uvicorn
import os
from cesar_tools_core import WreckHunterCore
from cesar_agent_llm import CesarSpecialistAgent

# Read role from environment to allow container specialization
# Roles: 'all', 'satellite_water', 'satellite_land', 'magnetics', 'sonar', 'sar', 'hyperspectral', 'biological', 'gravimetric', 'triple_lock'       
NODE_ROLE = os.getenv("CESAR_NODE_ROLE", "all").lower()

app = FastAPI(
    title=f"WreckHunter Microservice [{NODE_ROLE.upper()}]",
    description="Agentic node interface to WreckHunter tools with Local LLM reasoning."
)
core = WreckHunterCore()
ai_agent = CesarSpecialistAgent(NODE_ROLE)

class ScanAreaRequest(BaseModel):
    lat: float
    lon: float
    radius_km: float
    depth_profile: str = "shallow" # 'shallow', 'deep_500ft', 'land'
    date_start: str | None = None
    date_end: str | None = None

class BagFileRequest(BaseModel):
    filepath: str

# --- SATELLITE WATER CONTAINER ---
if NODE_ROLE in ["all", "satellite_water"]:
    @app.post("/api/v1/scan/satellite/water")
    def run_satellite_water_scan(req: ScanAreaRequest):
        """
        Specialist Endpoint: Deep water thermocline and cold-sink detection.    
        """
        try:
            results = core.run_satellite_ml_scan(
                lat=req.lat, lon=req.lon, radius_km=req.radius_km, depth_profile=req.depth_profile
            )
            # Route mathematical results through local LLM reasoner
            llm_result = ai_agent.analyze_results(results, "Deep Water Thermocline Satellite")
            return llm_result
        except Exception as e:
            raise HTTPException(status_code=500, detail=str(e))

# --- SATELLITE LAND CONTAINER ---
if NODE_ROLE in ["all", "satellite_land"]:
    @app.post("/api/v1/scan/satellite/land")
    def run_satellite_land_scan(req: ScanAreaRequest):
        try:
            results = core.run_satellite_scan(req.lat, req.lon, req.radius_km, (req.date_start, req.date_end))
            return ai_agent.analyze_results(results, "Land / Jungle NDVI Disturbance Satellite")
        except Exception as e:
            raise HTTPException(status_code=500, detail=str(e))

# --- MAGNETICS CONTAINER ---
if NODE_ROLE in ["all", "magnetics"]:
    @app.post("/api/v1/scan/mag")
    def run_mag_scan(req: ScanAreaRequest):
        try:
            results = core.run_mag_pipeline(req.lat, req.lon, req.radius_km)    
            return ai_agent.analyze_results(results, "Aerial Magnetic Dipole")
        except Exception as e:
            raise HTTPException(status_code=500, detail=str(e))

# --- SONAR CONTAINER ---
if NODE_ROLE in ["all", "bathymetry"]:
    @app.post("/api/v1/scan/bag")
    def run_bag_scan(req: BagFileRequest):
        try:
            results = core.analyze_bag_file(req.filepath)
            return ai_agent.analyze_results(results, "Multibeam Sonar / Coastal Lidar / BAG Geometrics")
        except Exception as e:
            raise HTTPException(status_code=500, detail=str(e))

# --- Synthetic Aperture Radar (SAR) ---
if NODE_ROLE in ["all", "sar"]:
    @app.post("/api/v1/scan/sar")
    def run_sar_scan(req: ScanAreaRequest):
        try:
            results = core.run_sar_pipeline(req.lat, req.lon, req.radius_km)
            return ai_agent.analyze_results(results, "SAR Backscatter and Corner Reflectors")
        except Exception as e:
            raise HTTPException(status_code=500, detail=str(e))

# --- Hyperspectral Chemical ---
if NODE_ROLE in ["all", "hyperspectral"]:
    @app.post("/api/v1/scan/hyperspectral")
    def run_hyperspectral_scan(req: ScanAreaRequest):
        try:
            results = core.run_hyperspectral_pipeline(req.lat, req.lon, req.radius_km)
            return ai_agent.analyze_results(results, "Galvanic / Chemical Plume Hyperspectral")
        except Exception as e:
            raise HTTPException(status_code=500, detail=str(e))

# --- Biological / Chlorophyll / Algae ---
if NODE_ROLE in ["all", "biological"]:
    @app.post("/api/v1/scan/biological")
    def run_biological_scan(req: ScanAreaRequest):
        try:
            results = core.run_biological_pipeline(req.lat, req.lon, req.radius_km)
            return ai_agent.analyze_results(results, "Biological Indicator / Chlorophyll Concentrates")
        except Exception as e:
            raise HTTPException(status_code=500, detail=str(e))

# --- Gravimetric (GOCE/GRACE) ---
if NODE_ROLE in ["all", "gravimetric"]:
    @app.post("/api/v1/scan/gravimetric")
    def run_gravimetric_scan(req: ScanAreaRequest):
        try:
            results = core.run_gravimetric_pipeline(req.lat, req.lon, req.radius_km)
            return ai_agent.analyze_results(results, "Bouguer Micro-gravity Anomalies")
        except Exception as e:
            raise HTTPException(status_code=500, detail=str(e))

# --- Triple Lock ---
if NODE_ROLE in ["all", "triple_lock"]:
    @app.post("/api/v1/scan/triple-lock")
    def execute_triple_lock(req: ScanAreaRequest):
        try:
            results = core.run_triple_lock_pipeline(req.lat, req.lon, req.radius_km, req.date_start, req.date_end)
            analysis = ai_agent.analyze_results(results, "Triple-Lock Fusion of Top 5 candidates spanning multiple years. Rule: >=3 positive sensor matches = confirmed wreck.")
            return analysis
        except Exception as e:
            raise HTTPException(status_code=500, detail=str(e))

if __name__ == "__main__":
    port_default = os.getenv("PORT", "8000")
    uvicorn.run(app, host="0.0.0.0", port=int(port_default))
