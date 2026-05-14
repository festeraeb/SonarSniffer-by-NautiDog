# CESAROPS AI Coding Instructions

## Project Overview
CESAROPS is a Great Lakes Search and Rescue (SAR) drift modeling system combining OpenDrift Lagrangian particle tracking with machine learning enhancements. It specializes in offline-capable SAR operations across all five Great Lakes.

## Architecture & Key Components

### Two-Tier Application Structure
- **`sarops.py`**: Full-featured GUI application with ML enhancements, data management, and visualization
- **`cesarops.py`**: Lightweight console script for basic drift simulations using OpenDrift

### Core Data Flow
1. **Data Ingestion**: Multi-source environmental data (GLERL ERDDAP, NDBC buoys, NOAA CO-OPS, NWS, USGS streams)
2. **Storage**: SQLite database (`drift_objects.db`) for offline operations and training data
3. **ML Pipeline**: Buoy track analysis → training data collection → drift correction models
4. **Simulation**: OpenDrift + ML corrections → KML/HTML outputs

### Critical Classes
- **`EnhancedOceanDrift`**: Extends OpenDrift with ML corrections and Great Lakes optimizations
- **`SAROpsApp`**: Main Tkinter GUI application with threaded operations

## Development Patterns

### Data Source Integration
All data fetchers follow the pattern: `fetch_*_data(db_file='drift_objects.db')` and store results in SQLite. Use the robust error handling pattern from existing fetchers when adding new sources.

### Environment Data Configuration
Great Lakes regions are defined in `config.yaml` with ERDDAP URLs and bounding boxes. The GLERL ERDDAP endpoints use specific parameter patterns:
```python
# Example: Lake Michigan currents
'michigan': 'https://coastwatch.glerl.noaa.gov/erddap/griddap/GLCFS_MICHIGAN_3D.nc?U[(last)][(0.0)][(41.5):(46.0)][(-88.5):(-85.5)],V[(last)][(0.0)][(41.5):(46.0)][(-88.5):(-85.5)]'
```

### ML Enhancement Workflow
1. `fetch_buoy_specifications()` - Get buoy metadata from NDBC
2. `fetch_historical_buoy_tracks()` - Collect actual drift tracks
3. `collect_ml_training_data()` - Correlate tracks with environmental conditions
4. `train_drift_correction_model()` - Train RandomForest correction model

### Dependency Management
The codebase uses defensive imports with feature flags (e.g., `TKINTER_AVAILABLE`, `SKLEARN_AVAILABLE`) to gracefully degrade when optional dependencies are missing. Always follow this pattern for new dependencies.

## Key Commands & Workflows

### Development Setup
```bash
# Conda environment (recommended)
launch_cesarops.bat  # Windows launcher with conda activation
# OR manual setup
conda activate cesarops
python sarops.py
```

### Testing & Validation
```bash
python test_ml_enhancements.py  # Test all ML features
python cesarops.py              # Test basic drift simulation
```

### Data Updates
```python
from sarops import auto_update_all_data
auto_update_all_data()  # Updates all environmental data sources
```

## Great Lakes Specifics

### Lake Boundaries (from config.yaml)
- **Michigan**: `[-88.5, -85.5, 41.5, 46.0]` (most common test case)
- **Erie**: `[-83.7, -78.8, 41.2, 42.9]`
- **Huron**: `[-85.0, -80.5, 43.2, 46.3]`
- **Ontario**: `[-80.0, -76.0, 43.2, 44.3]`
- **Superior**: `[-92.5, -84.5, 46.0, 49.5]`

### Default Simulation Parameters
- **Duration**: 24 hours, **Timestep**: 10 minutes
- **Windage**: 0.03, **Stokes drift**: 0.01
- **Seeding**: 2.0 nm radius, 60 particles/hour

## File Organization Conventions

### Directory Structure
- `data/`: NetCDF environmental data files (auto-managed)
- `outputs/`: Simulation results (HTML/KML formats)
- `models/`: Trained ML models (joblib pickles)
- `logs/`: Application logs

### Naming Patterns
- Data files: `{source}_{lake}_{timestamp}.nc` (e.g., `gl_michigan_20251007_155312.nc`)
- Output files: `sim_{lake}_{timestamp}.{html|kml}`
- Models: `drift_correction_model.pkl`

## Important Notes

### Case Study Reference
The "Charlie Brown case" (Milwaukee to South Haven crossing) is used as a validation benchmark. Located in `analyze_charlie_brown_case()` - use this pattern for adding new validation cases.

### OpenDrift Integration
Always use `safe_open_netcdf()` for robust NetCDF handling with fallbacks. The system is designed to work even when OpenDrift is not available (graceful degradation).

### Threading Requirements
GUI operations use threading extensively. All long-running operations (data fetching, simulations) must run in separate threads to prevent UI freezing.