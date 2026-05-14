# CESAROPS - Civilian Emergency SAR Operations

**Enhanced Drift Modeling System for Search and Rescue Operations**

CESAROPS is a free, open-source drift modeling tool designed specifically for Search and Rescue (SAR) volunteer organizations. It provides robust, offline-capable drift predictions using oceanographic data and optional machine learning enhancement.

## 🌊 Features

### Core Capabilities
- **Multi-source ocean current data** - LMHOFS, RTOFS, HYCOM support
- **Robust data fetching** - Retry logic, fallback to cached data
- **Flexible particle seeding** - Circular, line, and custom patterns  
- **Forward and backward drift modeling** - Track objects forward or backtrack from found location
- **Offline operation** - Cached data and local storage for field use
- **Export formats** - CSV, KML, and comprehensive reports

### Advanced Features
- **Machine Learning enhancement** - Optional ML corrections to physics models
- **Performance tracking** - Model accuracy metrics and validation
- **Multi-threading** - Non-blocking GUI with background processing
- **Comprehensive logging** - Full activity logging for analysis
- **Portable design** - Self-contained with minimal dependencies

### Physical Factors Modeled
- Ocean/lake surface currents
- Windage effects (surface wind drag)
- Stokes drift (wave-induced transport)
- Configurable object properties

## 🚀 Quick Start

### Windows Installation

1. **Download and extract** CESAROPS files to a folder
2. **Run the installer**: Double-click `install_cesarops.bat`
3. **Follow prompts** to install optional components
4. **Launch**: Double-click `run_cesarops.bat`

### Manual Installation

```bash
# Clone or download the project
git clone <repository-url>
cd cesarops

# Create virtual environment
python -m venv venv
source venv/bin/activate  # Linux/Mac
# or
venv\Scripts\activate     # Windows

# Install dependencies
pip install -r requirements.txt

# Run application
python cesarops_enhanced.py
```

## 📋 System Requirements

### Minimum Requirements
- Python 3.8 or later
- 4 GB RAM
- 1 GB disk space
- Internet connection for data fetching

### Recommended
- Python 3.10+
- 8 GB RAM (for large simulations)
- SSD storage (faster data processing)
- Reliable internet (for real-time data)

### Optional Components
- **scikit-learn** + **joblib** - Machine learning enhancement
- **simplekml** - KML export support
- **matplotlib** - Enhanced plotting capabilities

## 🎯 Usage Guide

### 1. Data Sources Tab
- **Select data source** (LMHOFS for Great Lakes, RTOFS/HYCOM for oceans)
- **Set time range** (UTC timestamps)
- **Define bounding box** (West, East, South, North coordinates)
- **Fetch data** or use cached data for offline operation

### 2. Seed Particles Tab
- **Circular seeding**: Specify center point and radius for area searches
- **Line seeding**: Create search patterns along transects
- **Configure timing**: Start/end times and particle release rate

### 3. Run Simulation Tab
- **Set parameters**: Time step, duration, windage, Stokes drift
- **Enable ML** (if available) for enhanced predictions
- **Run forward drift** or **backtrack simulation**
- Monitor progress in real-time

### 4. Results Tab
- **View simulation summary** and statistics
- **Export to CSV** for GIS analysis
- **Export to KML** for Google Earth visualization
- **Generate reports** for SAR documentation

### 5. Settings Tab
- **Configure data sources** and endpoints
- **Adjust performance settings**
- **Save configuration** for team standardization

## 🧠 Machine Learning Enhancement

CESAROPS includes optional ML enhancement that learns from historical drift patterns to improve predictions:

### Training the Model
1. Run multiple simulations to build training data
2. Use the "Train ML Model" button
3. Model automatically validates performance
4. Enhanced predictions used in future simulations

### Benefits
- **Improved accuracy** for local conditions
- **Learns systematic biases** in physics models  
- **Adapts to regional patterns** over time
- **Quantified performance** metrics

## 📊 Data Sources

### Supported Ocean Models
- **LMHOFS** - Lake Michigan Hydrodynamic and Forecast System
- **RTOFS** - Real-Time Ocean Forecast System (NOAA)
- **HYCOM** - Hybrid Coordinate Ocean Model

### Data Quality Features
- **Automatic failover** between data sources
- **Quality checking** and outlier detection
- **Caching system** for offline capability
- **Data validation** and error handling

## 🗂️ File Structure

```
cesarops/
├── cesarops_enhanced.py    # Main application
├── config.yaml            # Configuration file
├── requirements.txt        # Python dependencies
├── install_cesarops.bat    # Windows installer
├── run_cesarops.bat       # Launch script
├── data/                  # Cached ocean data
├── outputs/               # Simulation results
├── models/                # ML model files
├── logs/                  # Application logs
└── README.md             # This file
```

## ⚙️ Configuration

Edit `config.yaml` to customize:

```yaml
erddap:
  lmhofs: "https://coastwatch.glerl.noaa.gov/erddap"
  rtofs: "https://coastwatch.pfeg.noaa.gov/erddap"
  hycom: "https://tds.hycom.org/erddap"

drift_defaults:
  dt_minutes: 10        # Time step (minutes)
  duration_hours: 24    # Simulation duration
  windage: 0.03        # Windage factor (0-0.1 typical)
  stokes: 0.01         # Stokes drift factor

seeding:
  default_radius_nm: 2.0  # Default search radius (nautical miles)
  default_rate: 60        # Seeds per hour
```

## 🎯 Use Cases

### Search and Rescue Operations
- **Person overboard** - Model drift from last known position
- **Missing vessel** - Backtrack from debris field
- **Aviation SAR** - Ocean survival scenarios
- **Mass rescue** - Multiple casualty drift patterns

### Planning and Training
- **SAR exercise planning** - Realistic scenarios
- **Resource positioning** - Optimize asset placement
- **Training scenarios** - Educational drift modeling
- **Risk assessment** - Evaluate drift hazards

### Research Applications  
- **Ocean dynamics study** - Validate circulation models
- **Pollution tracking** - Spill trajectory analysis
- **Marine biology** - Larval transport studies
- **Climate research** - Long-term drift patterns

## 🔧 Troubleshooting

### Common Issues

**"No data available"**
- Check internet connection
- Verify time range (not too far in past/future)
- Try different data source
- Check bounding box coordinates

**GUI freezing**
- Large simulations run in background - wait for completion
- Check system memory usage
- Reduce simulation duration or particle count

**ML features not working**
- Install optional ML dependencies: `pip install scikit-learn joblib`
- Generate training data by running multiple simulations
- Check logs for ML-specific errors

**Export failures**
- Ensure output directory exists and is writable
- For KML export, install: `pip install simplekml`
- Check disk space availability

### Performance Optimization

**For large simulations:**
- Increase time step (reduce computational load)
- Reduce particle count
- Limit simulation duration
- Use cached data when possible

**For better accuracy:**
- Decrease time step (10 minutes or less)
- Use recent, high-resolution data
- Enable ML enhancement
- Validate with known drift cases

## 📚 Scientific Background

### Drift Modeling Theory
CESAROPS implements Lagrangian particle tracking with the following physics:

```
dx/dt = u_current + u_wind * windage + u_stokes
dy/dt = v_current + v_wind * windage + v_stokes
```

Where:
- `u,v_current` - Ocean/lake surface currents
- `windage` - Wind drag coefficient (typically 0.01-0.05)
- `u,v_stokes` - Stokes drift from surface waves

### Coordinate System
- **Geographic coordinates** (latitude, longitude)  
- **Earth radius**: 6,371,000 m
- **Projection effects** handled for accurate distances

### Time Integration
- **Forward Euler** scheme for particle advancement
- **Adaptive time stepping** for numerical stability
- **Configurable time steps** (1-60 minutes typical)

## 🤝 Contributing

CESAROPS is open source and welcomes contributions:

### Development Setup
1. Fork the repository
2. Create virtual environment
3. Install development dependencies
4. Make changes and test thoroughly
5. Submit pull request

### Areas for Contribution
- **Additional data sources** (new ERDDAP endpoints)
- **Enhanced ML models** (neural networks, ensemble methods)
- **Visualization improvements** (real-time plotting)
- **Mobile interfaces** (tablet-friendly GUI)
- **Performance optimization** (parallel processing)

## 📄 License

This project is released under the MIT License, making it free for all SAR organizations and humanitarian use.

## 🆘 Support

### For SAR Organizations
- **Training support** available for teams
- **Customization** for local conditions
- **Integration** with existing SAR systems

### Getting Help
- Check this README and logs first
- Create detailed issue reports
- Include system information and error messages
- Provide sample data when possible

## 🏆 Acknowledgments

- **NOAA/GLERL** - Great Lakes ocean model data
- **NOAA/NWS** - Ocean forecast systems
- **ERDDAP** - Data server technology
- **SAR communities** - Requirements and validation
- **Open source contributors** - Code and testing

## 🔮 Roadmap

### Planned Features
- **Real-time weather integration** (wind data)
- **Ensemble forecasting** (uncertainty quantification)
- **Mobile app version** (field operations)
- **AIS integration** (vessel tracking)
- **Web-based interface** (multi-user access)

### Long-term Vision
- **AI-powered SAR assistant** - Automated decision support
- **Global coverage** - Worldwide ocean/lake support
- **Sensor integration** - Real-time drifter data
- **Interoperability** - Standards-based data exchange

---

**CESAROPS - Saving Lives Through Better Science**

*For emergency SAR operations, always follow established protocols and use CESAROPS predictions as one tool among many in your decision-making process.*