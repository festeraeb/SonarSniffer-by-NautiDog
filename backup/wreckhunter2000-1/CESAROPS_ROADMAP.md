# CESAROPS WreckHunter2000 Roadmap
**The "Altered Physics" Detection Stack**

This document serves as the master tracking roadmap for our containerized "Triple-Lock" verification architecture across the Deep Search Stack. This ensures no logic is lost when context windows renew.

## 1. System Architecture: "Brain & Muscle" Containerization
- **The Brain (Orchestrator)**: Python LLM agents running on the 16GB i7 host. Handles JSON inputs, evaluates anomaly outputs, manages STAC routing.
- **The Muscle (Specialists)**: Rust-compiled, CUDA-accelerated binaries inside Docker, deployed to Dual 16GB Nvidia P100s. Highly specialized math.
- **Zero-Disk Pipeline**: Use STAC-client and copc-streaming to bypass hard drive write latency entirely.

## 2. Detection Methodology: "Triple-Lock Verification"
A 3-tier spatial overlap system targeting anomalies in physics, biology, and chemistry:
1. **Optical/Topographical** (Bathymetry edges, spines, Lidar points).
2. **SAR** (Synthetic Aperture Radar) (Hydrocarbon slick tracking, biogenic wave damping).
3. **Thermal/Aeromagnetic** (Deep-water thermal inertia shimmers, iron signatures).

## 3. Deployment Phases & Ranking

### 🥇 Tier 1: Immediate Execution (Highest Feasibility)
- [ ] **The Biogenic Slick Tracker (Sentinel-1 SAR / stac-client + scirs2-vision)**
  - *Goal*: Isolate non-moving "black" patches caused by galvanic ion release/corrosion and organic surfactants damping capillary waves on the surface.
- [ ] **The "Spine" Curvelet Extractor (Optical / nauticuvs)**
  - *Goal*: Run `nauticuvs` Fast Discrete Curvelet Transforms on Sentinel-2 B01 (Coastal Blue) to extract sharp anisotropic lines ("spines") from chaotic water pixels.
- [ ] **The Artificial Reef Locator (Sentinel-3 OLCI / scirs2-ndimage)**
  - *Goal*: Detect persistent Chlorophyll-a / algae green spots caused by wrecks acting as biological anchors.

### 🥈 Tier 2: Physical Anomaly Hunters & AI Fusion (High Likelihood, Needs Perfect Conditions)
- [ ] **Data Fusion & AIS "Ghost Ship" Correlator**
  - *Goal*: Fuse SAR (blob detection) and Optical (color/texture) with AIS "dark ship" / ADS-B discontinuities. Implements Bayesian association to filter false alarms and flag transient targets lacking transponder data.
- [ ] **CNN Multimodal Detector (YOLO/Faster-RCNN)**
  - *Goal*: Supervised object detection trained on labeled wreck/debris examples (edges/HOG features combined with bright SAR metal returns).
- [ ] **Thermal Inertia Lag Detector (VIIRS / Landsat TIR / satellite Vu)**
  - *Goal*: Compare daytime vs nighttime thermal imagery to spot heat anomalies or pixels that lag in temperature shift. Useful for active crash sites or thermal variations over large submerged structures.

### 🥉 Tier 3: Bleeding-Edge Specialists (Tasking & Archival Exploitation)
- [ ] **Chemical Fingerprint / Spectral Unmixing (PRISMA / DESIS Hyperspectral)**
  - *Goal*: Isolate unusual spectral signatures (oil, exposed metal, or vegetation stress).
- [ ] **"Water Mound" Profiler (ICESat-2 ATL03 / hdf5 / netcdf)**
  - *Goal*: Use green-light photon returns to find centimeter-scale topographic surface mounds indicative of deep-water current obstruction.
- [ ] **Automated Tasking Engine**
  - *Goal*: Query taskable commercial satellites (Maxar, Planet, ICEYE, Capella) when archival (Sentinel, Landsat) data flags a high-priority low-res anomaly that requires sub-meter verification.

## 4. Operational Sidecar Workflow
A modular microservices architecture linking with the main tracking system:
1. **Ingestion Service**: Poll APIs (Planet, Sentinel Hub, AIS feeds).
2. **Processing Service**: Apply atmospheric correction, orthorectification, speckle filtering, and detection models (Rust workers).
3. **Database/Queue**: Store candidate events with confidence scores.
4. **Scoring/Fusion Service**: Aggregate evidence (image score + AIS gap + drift model likelihood).
5. **Alert Service**: Push geo-coordinates (GeoJSON/KML alerts) to analysts.
