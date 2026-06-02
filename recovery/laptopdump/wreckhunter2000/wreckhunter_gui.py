"""
wreckhunter_gui.py

WreckHunter 2000 — Satellite Scanning Control Panel
PyQt5 GUI for Great Lakes satellite scanning operations.

Features:
  - Scenario checkboxes (multi-select)
  - Date range with bypass toggles
  - Detection stack toggles (Thermal, SAR, Reverse Thermal, etc.)
  - Scan modes: Complete Set, Custom, Single-Day Event, Sink Date Comparison
  - Location input: Center+Radius or Custom Bounding Box
  - Real-time scan queue preview
  - CUDA status indicator

CUDA-AccelerATED: Quadro M2200
"""

import sys
import json
from datetime import datetime, timedelta
from pathlib import Path

from PyQt5.QtWidgets import (
    QApplication, QMainWindow, QWidget, QVBoxLayout, QHBoxLayout,
    QLabel, QPushButton, QCheckBox, QComboBox, QDateEdit, QSpinBox,
    QDoubleSpinBox, QGroupBox, QGridLayout, QTabWidget, QTextEdit,
    QProgressBar, QRadioButton, QButtonGroup, QMessageBox, QFileDialog
)
from PyQt5.QtCore import Qt, QDate, QThread, pyqtSignal
from PyQt5.QtGui import QFont, QColor, QPalette

# Import scanning modules
from great_lakes_scanner import (
    GREAT_LAKES_BBOXES, SCAN_SCENARIOS, run_great_lakes_scan,
    get_scan_history, init_scan_registry, get_optimal_scan_dates
)
from satellite_target_fetcher import run_satellite_target_fetch

# ── CUDA Status ───────────────────────────────────────────────────────────────

try:
    import torch
    CUDA_AVAILABLE = torch.cuda.is_available()
    GPU_NAME = torch.cuda.get_device_name(0) if CUDA_AVAILABLE else "N/A"
except ImportError:
    CUDA_AVAILABLE = False
    GPU_NAME = "N/A"

# ── Worker Thread ─────────────────────────────────────────────────────────────

class ScanWorker(QThread):
    """Background worker for scan operations."""
    progress = pyqtSignal(str)
    finished = pyqtSignal(dict)
    error = pyqtSignal(str)
    
    def __init__(self, scan_params: dict):
        super().__init__()
        self.params = scan_params
    
    def run(self):
        try:
            self.progress.emit("Initializing scan registry...")
            init_scan_registry()
            
            # Run scan based on mode
            if self.params['mode'] == 'complete_set':
                result = self._run_complete_set()
            elif self.params['mode'] == 'single_day_event':
                result = self._run_single_day()
            elif self.params['mode'] == 'sink_date':
                result = self._run_sink_date()
            else:
                result = self._run_custom()
            
            self.finished.emit(result)
            
        except Exception as e:
            import traceback
            self.error.emit(f"{str(e)}\n\n{traceback.format_exc()}")
    
    def _run_complete_set(self):
        """Run all selected scenarios for selected lakes."""
        results = []
        lakes = self.params['lakes']
        scenarios = self.params['scenarios']
        
        total = len(lakes) * len(scenarios)
        scan_count = 0
        
        for i, lake in enumerate(lakes):
            for j, scenario in enumerate(scenarios):
                scan_count += 1
                idx = scan_count
                self.progress.emit(f"[{idx}/{total}] Scanning {lake} - {scenario}...")
                
                # Get optimal dates for this lake/scenario
                from great_lakes_scanner import get_optimal_scan_dates
                
                # Determine year range (auto low-water or manual)
                if self.params['start_date'] is None:
                    # Auto: use low-water years for this lake
                    lake_info = GREAT_LAKES_BBOXES.get(lake, {})
                    low_years = lake_info.get('low_water_years', [2021])
                    year_range = (max(low_years) - 2, max(low_years))
                else:
                    start_year = int(self.params['start_date'][:4])
                    end_year = int(self.params['end_date'][:4])
                    year_range = (start_year, end_year)
                
                # Get required sensors based on detection stacks
                required_sensors = ['sentinel2a']  # Always need S2
                if self.params['detection_stacks']['thermal']:
                    required_sensors.append('landsat8')
                if self.params['detection_stacks']['swot']:
                    required_sensors.append('swot')
                if self.params['detection_stacks']['sar']:
                    required_sensors.append('sentinel1a')
                
                # Get optimal dates
                opt_result = get_optimal_scan_dates(
                    lake_region=lake,
                    year_range=year_range,
                    required_sensors=required_sensors,
                    min_panels_per_sensor=5,
                )
                
                # Run the actual scan for best dates
                if opt_result['best_dates']:
                    best_date = opt_result['best_dates'][0]['date']
                    
                    result = run_great_lakes_scan(
                        lake_region=lake,
                        scenario=scenario,
                        start_date=best_date,
                        end_date=best_date,  # Single day for now
                        output_format='kmz',
                    )
                    
                    result['optimal_dates_info'] = opt_result
                    results.append(result)
                else:
                    results.append({
                        'lake': lake,
                        'scenario': scenario,
                        'status': 'NO_COVERAGE',
                        'warnings': opt_result['warnings'],
                    })
        
        return {'scans': results, 'total': len(results)}
    
    def _run_single_day(self):
        """Single day event scan with center+radius or bbox."""
        # TODO: Implement single-day scan logic
        return {'status': 'TODO_IMPLEMENT'}
    
    def _run_sink_date(self):
        """Sink date comparison (before/after event)."""
        # TODO: Implement sink date comparison
        return {'status': 'TODO_IMPLEMENT'}
    
    def _run_custom(self):
        """Custom scan with selected detection stacks."""
        # TODO: Implement custom scan
        return {'status': 'TODO_IMPLEMENT'}


# ── Main GUI ──────────────────────────────────────────────────────────────────

class WreckHunterGUI(QMainWindow):
    """Main application window."""
    
    def __init__(self):
        super().__init__()
        self.worker = None
        self.init_ui()
        self.update_queue_preview()
    
    def init_ui(self):
        """Build the GUI layout."""
        self.setWindowTitle("WreckHunter 2000 — Satellite Scanning Control Panel")
        self.setMinimumSize(1200, 800)
        
        # Central widget
        central = QWidget()
        self.setCentralWidget(central)
        main_layout = QVBoxLayout(central)
        
        # ── Header ────────────────────────────────────────────────────────────
        header = QHBoxLayout()
        
        title = QLabel("🛰️ WreckHunter 2000")
        title.setFont(QFont("Arial", 18, QFont.Bold))
        header.addWidget(title)
        
        # CUDA status
        cuda_status = "✅" if CUDA_AVAILABLE else "❌"
        cuda_label = QLabel(f"{cuda_status} CUDA: {GPU_NAME}" if CUDA_AVAILABLE else f"{cuda_status} CPU Only")
        cuda_label.setFont(QFont("Arial", 10))
        if CUDA_AVAILABLE:
            cuda_label.setStyleSheet("color: green;")
        else:
            cuda_label.setStyleSheet("color: orange;")
        header.addWidget(cuda_label)
        
        header.addStretch()
        main_layout.addLayout(header)
        
        # ── Tab Widget ────────────────────────────────────────────────────────
        tabs = QTabWidget()
        main_layout.addWidget(tabs)
        
        # Tab 1: Scan Configuration
        tab_config = QWidget()
        tabs.addTab(tab_config, "🔧 Scan Configuration")
        config_layout = QVBoxLayout(tab_config)
        
        # ── Scan Mode Selection ──────────────────────────────────────────────
        mode_group = QGroupBox("Scan Mode")
        mode_layout = QVBoxLayout()
        
        self.mode_complete = QRadioButton("Complete Set Run (All Selected Scenarios + Lakes)")
        self.mode_complete.setChecked(True)
        self.mode_complete.toggled.connect(self.update_queue_preview)
        
        self.mode_custom = QRadioButton("Custom (Select Detection Stacks)")
        self.mode_custom.toggled.connect(self.update_queue_preview)
        
        self.mode_single_day = QRadioButton("Single Day Event (Center + Radius / Custom BBox)")
        self.mode_single_day.toggled.connect(self.update_queue_preview)
        
        self.mode_sink_date = QRadioButton("Sink Date Comparison (Before/After Event)")
        self.mode_sink_date.toggled.connect(self.update_queue_preview)
        
        mode_layout.addWidget(self.mode_complete)
        mode_layout.addWidget(self.mode_custom)
        mode_layout.addWidget(self.mode_single_day)
        mode_layout.addWidget(self.mode_sink_date)
        mode_group.setLayout(mode_layout)
        config_layout.addWidget(mode_group)
        
        # ── Lake Selection ────────────────────────────────────────────────────
        lake_group = QGroupBox("Great Lakes / Regions")
        lake_layout = QGridLayout()
        
        self.lake_checks = {}
        lakes = list(GREAT_LAKES_BBOXES.keys())
        cols = 3
        for i, lake in enumerate(lakes):
            cb = QCheckBox(GREAT_LAKES_BBOXES[lake]['name'])
            cb.setChecked(True)  # Default all on
            cb.toggled.connect(self.update_queue_preview)
            self.lake_checks[lake] = cb
            row, col = divmod(i, cols)
            lake_layout.addWidget(cb, row, col)
        
        lake_group.setLayout(lake_layout)
        config_layout.addWidget(lake_group)
        
        # ── Scenario Selection ────────────────────────────────────────────────
        scenario_group = QGroupBox("Scan Scenarios")
        scenario_layout = QGridLayout()
        
        self.scenario_checks = {}
        for i, (key, scenario) in enumerate(SCAN_SCENARIOS.items()):
            cb = QCheckBox(f"{scenario['name']} (Priority: {scenario['priority_weight']}×)")
            cb.setToolTip(scenario['description'])
            cb.setChecked(True)
            cb.toggled.connect(self.update_queue_preview)
            self.scenario_checks[key] = cb
            row, col = divmod(i, 2)
            scenario_layout.addWidget(cb, row, col)
        
        scenario_group.setLayout(scenario_layout)
        config_layout.addWidget(scenario_group)
        
        # ── Detection Stacks (Custom Mode) ────────────────────────────────────
        detection_group = QGroupBox("Detection Stacks (Custom Mode Only)")
        detection_layout = QHBoxLayout()
        
        self.detect_thermal = QCheckBox("Thermal (Hot Day → Cold Night)")
        self.detect_thermal.setChecked(True)
        self.detect_thermal.toggled.connect(self.update_queue_preview)
        
        self.detect_reverse_thermal = QCheckBox("Reverse Thermal (Lead-Hunter) — ALWAYS ACTIVE\nFRP-Encapsulated Lead Keels (Ice Cube in Thermos)")
        self.detect_reverse_thermal.setChecked(True)
        self.detect_reverse_thermal.setEnabled(False)  # Always on, can't be disabled
        self.detect_reverse_thermal.setToolTip(
            "ALWAYS ACTIVE for all scans.\n\n"
            "Detects FRP-encapsulated lead keels.\n"
            "Physics: Lead stays at 4°C while surface warms.\n"
            "Fiberglass acts as 'thermos' preserving cold signature.\n"
            "Appears as persistent negative Z-score cold spot.\n\n"
            "Classification:\n"
            "  • Cold spike ONLY = Rossa (new FRP vessel)\n"
            "  • Cold spike + Mussel Glow = Andaste (historical steel)"
        )
        
        self.detect_sar = QCheckBox("SAR (Synthetic Aperture Radar)\nMetal/Fabric/Chromoly Detection")
        self.detect_sar.setChecked(True)
        self.detect_sar.setToolTip(
            "Detects aluminum aircraft wreckage, fabric surfaces,\n"
            "and chromoly tubing via radar signature differences."
        )
        self.detect_sar.toggled.connect(self.update_queue_preview)
        
        self.detect_optical = QCheckBox("Optical (Shadow/Roughness/Mussel Glow)")
        self.detect_optical.setChecked(True)
        self.detect_optical.toggled.connect(self.update_queue_preview)
        
        self.detect_swot = QCheckBox("SWOT (Height Anomalies >1cm)")
        self.detect_swot.setChecked(True)
        self.detect_swot.toggled.connect(self.update_queue_preview)
        
        detection_layout.addWidget(self.detect_thermal)
        detection_layout.addWidget(self.detect_sar)
        detection_layout.addWidget(self.detect_reverse_thermal)
        detection_layout.addWidget(self.detect_optical)
        detection_layout.addWidget(self.detect_swot)
        detection_group.setLayout(detection_layout)
        config_layout.addWidget(detection_group)
        
        # ── Date Range ────────────────────────────────────────────────────────
        date_group = QGroupBox("Date Range")
        date_layout = QGridLayout()
        
        # Auto low-water checkbox
        self.date_auto = QCheckBox("Auto Low-Water Years (Recommended)")
        self.date_auto.setChecked(True)
        self.date_auto.stateChanged.connect(self.toggle_date_inputs)
        date_layout.addWidget(self.date_auto, 0, 0, 1, 2)
        
        # Start date
        date_layout.addWidget(QLabel("Start Date:"), 1, 0)
        self.start_date = QDateEdit()
        self.start_date.setCalendarPopup(True)
        self.start_date.setDate(QDate(2021, 6, 1))
        self.start_date.setEnabled(False)
        self.start_date.dateChanged.connect(self.update_queue_preview)
        date_layout.addWidget(self.start_date, 1, 1)
        
        # End date
        date_layout.addWidget(QLabel("End Date:"), 2, 0)
        self.end_date = QDateEdit()
        self.end_date.setCalendarPopup(True)
        self.end_date.setDate(QDate(2021, 9, 30))
        self.end_date.setEnabled(False)
        self.end_date.dateChanged.connect(self.update_queue_preview)
        date_layout.addWidget(self.end_date, 2, 1)
        
        # Bypass toggle
        self.date_bypass = QCheckBox("Bypass Date Logic (Force Exact Range)")
        self.date_bypass.stateChanged.connect(self.update_queue_preview)
        date_layout.addWidget(self.date_bypass, 3, 0, 1, 2)
        
        date_group.setLayout(date_layout)
        config_layout.addWidget(date_group)
        
        # ── Location Input (Single Day / Sink Date Modes) ────────────────────
        location_group = QGroupBox("Location (Single Day / Sink Date Modes)")
        location_layout = QGridLayout()
        
        # Center + Radius vs BBox
        self.loc_center_radio = QRadioButton("Center + Radius")
        self.loc_center_radio.setChecked(True)
        location_layout.addWidget(self.loc_center_radio, 0, 0)
        
        self.loc_bbox_radio = QRadioButton("Custom Bounding Box")
        location_layout.addWidget(self.loc_bbox_radio, 0, 1)
        
        # Center lat/lon
        location_layout.addWidget(QLabel("Center Lat:"), 1, 0)
        self.center_lat = QDoubleSpinBox()
        self.center_lat.setRange(-90, 90)
        self.center_lat.setDecimals(4)
        self.center_lat.setValue(42.465)
        self.center_lat.setEnabled(False)
        location_layout.addWidget(self.center_lat, 1, 1)
        
        location_layout.addWidget(QLabel("Center Lon:"), 2, 0)
        self.center_lon = QDoubleSpinBox()
        self.center_lon.setRange(-180, 180)
        self.center_lon.setDecimals(4)
        self.center_lon.setValue(-87.105)
        self.center_lon.setEnabled(False)
        location_layout.addWidget(self.center_lon, 2, 1)
        
        location_layout.addWidget(QLabel("Radius (km):"), 3, 0)
        self.radius_km = QSpinBox()
        self.radius_km.setRange(1, 500)
        self.radius_km.setValue(50)
        self.radius_km.setEnabled(False)
        location_layout.addWidget(self.radius_km, 3, 1)
        
        # BBox inputs
        location_layout.addWidget(QLabel("BBox W:"), 4, 0)
        self.bbox_w = QDoubleSpinBox()
        self.bbox_w.setRange(-180, 180)
        self.bbox_w.setDecimals(4)
        self.bbox_w.setValue(-87.15)
        self.bbox_w.setEnabled(False)
        location_layout.addWidget(self.bbox_w, 4, 1)
        
        location_layout.addWidget(QLabel("BBox S:"), 5, 0)
        self.bbox_s = QDoubleSpinBox()
        self.bbox_s.setRange(-90, 90)
        self.bbox_s.setDecimals(4)
        self.bbox_s.setValue(42.44)
        self.bbox_s.setEnabled(False)
        location_layout.addWidget(self.bbox_s, 5, 1)
        
        location_layout.addWidget(QLabel("BBox E:"), 6, 0)
        self.bbox_e = QDoubleSpinBox()
        self.bbox_e.setRange(-180, 180)
        self.bbox_e.setDecimals(4)
        self.bbox_e.setValue(-87.06)
        self.bbox_e.setEnabled(False)
        location_layout.addWidget(self.bbox_e, 6, 1)
        
        location_layout.addWidget(QLabel("BBox N:"), 7, 0)
        self.bbox_n = QDoubleSpinBox()
        self.bbox_n.setRange(-90, 90)
        self.bbox_n.setDecimals(4)
        self.bbox_n.setValue(42.49)
        self.bbox_n.setEnabled(False)
        location_layout.addWidget(self.bbox_n, 7, 1)
        
        location_group.setLayout(location_layout)
        config_layout.addWidget(location_group)
        
        # ── Sink Date Specific ────────────────────────────────────────────────
        sink_group = QGroupBox("Sink Date Comparison")
        sink_layout = QGridLayout()
        
        self.sink_date_check = QCheckBox("Enable Sink Date Comparison")
        self.sink_date_check.stateChanged.connect(self.update_queue_preview)
        sink_layout.addWidget(self.sink_date_check, 0, 0, 1, 2)
        
        sink_layout.addWidget(QLabel("Event/Sink Date:"), 1, 0)
        self.sink_date = QDateEdit()
        self.sink_date.setCalendarPopup(True)
        self.sink_date.setDate(QDate(2021, 7, 15))
        sink_layout.addWidget(self.sink_date, 1, 1)
        
        sink_layout.addWidget(QLabel("Days Before:"), 2, 0)
        self.sink_days_before = QSpinBox()
        self.sink_days_before.setRange(1, 30)
        self.sink_days_before.setValue(3)
        sink_layout.addWidget(self.sink_days_before, 2, 1)
        
        sink_layout.addWidget(QLabel("Days After:"), 3, 0)
        self.sink_days_after = QSpinBox()
        self.sink_days_after.setRange(1, 30)
        self.sink_days_after.setValue(3)
        sink_layout.addWidget(self.sink_days_after, 3, 1)
        
        sink_group.setLayout(sink_layout)
        config_layout.addWidget(sink_group)
        
        # ── Action Buttons ────────────────────────────────────────────────────
        button_layout = QHBoxLayout()
        
        self.btn_start = QPushButton("🚀 Start Scan")
        self.btn_start.setFont(QFont("Arial", 12, QFont.Bold))
        self.btn_start.setStyleSheet("background-color: #4CAF50; color: white; padding: 10px;")
        self.btn_start.clicked.connect(self.start_scan)
        button_layout.addWidget(self.btn_start)
        
        self.btn_preview = QPushButton("📋 Preview Queue")
        self.btn_preview.clicked.connect(self.update_queue_preview)
        button_layout.addWidget(self.btn_preview)
        
        self.btn_clear = QPushButton("🗑️ Clear")
        self.btn_clear.clicked.connect(self.clear_selections)
        button_layout.addWidget(self.btn_clear)
        
        config_layout.addLayout(button_layout)
        
        # ── Queue Preview ─────────────────────────────────────────────────────
        preview_group = QGroupBox("Scan Queue Preview")
        preview_layout = QVBoxLayout()
        
        self.queue_preview = QTextEdit()
        self.queue_preview.setReadOnly(True)
        self.queue_preview.setMaximumHeight(200)
        preview_layout.addWidget(self.queue_preview)
        
        self.progress_bar = QProgressBar()
        self.progress_bar.setVisible(False)
        preview_layout.addWidget(self.progress_bar)
        
        self.status_label = QLabel("Ready")
        preview_layout.addWidget(self.status_label)
        
        preview_group.setLayout(preview_layout)
        config_layout.addWidget(preview_group)
        
        # Tab 2: Scan History
        tab_history = QWidget()
        tabs.addTab(tab_history, "📊 Scan History")
        history_layout = QVBoxLayout(tab_history)
        
        self.history_view = QTextEdit()
        self.history_view.setReadOnly(True)
        history_layout.addWidget(self.history_view)
        
        btn_refresh = QPushButton("🔄 Refresh History")
        btn_refresh.clicked.connect(self.load_history)
        history_layout.addWidget(btn_refresh)
        
        # Tab 3: Settings
        tab_settings = QWidget()
        tabs.addTab(tab_settings, "⚙️ Settings")
        settings_layout = QVBoxLayout(tab_settings)
        
        settings_layout.addWidget(QLabel("Storage Limit (GB):"))
        self.storage_limit = QSpinBox()
        self.storage_limit.setRange(50, 1000)
        self.storage_limit.setValue(200)
        settings_layout.addWidget(self.storage_limit)
        
        settings_layout.addStretch()
        
        # ── Status Bar ────────────────────────────────────────────────────────
        self.statusBar().showMessage("Ready — Select scan parameters and click Start")
    
    def toggle_date_inputs(self, state):
        """Enable/disable date inputs based on auto checkbox."""
        enabled = (state == Qt.Unchecked)
        self.start_date.setEnabled(enabled)
        self.end_date.setEnabled(enabled)
        self.update_queue_preview()
    
    def get_scan_params(self):
        """Collect current GUI parameters."""
        # Get selected lakes
        lakes = [lake for lake, cb in self.lake_checks.items() if cb.isChecked()]
        
        # Get selected scenarios
        scenarios = [key for key, cb in self.scenario_checks.items() if cb.isChecked()]
        
        # Get dates
        if self.date_auto.isChecked():
            start_date = None
            end_date = None
        else:
            start_date = self.start_date.date().toString("yyyy-MM-dd")
            end_date = self.end_date.date().toString("yyyy-MM-dd")
        
        # Determine mode
        if self.mode_complete.isChecked():
            mode = 'complete_set'
        elif self.mode_custom.isChecked():
            mode = 'custom'
        elif self.mode_single_day.isChecked():
            mode = 'single_day_event'
        else:
            mode = 'sink_date'
        
        return {
            'mode': mode,
            'lakes': lakes,
            'scenarios': scenarios,
            'start_date': start_date,
            'end_date': end_date,
            'date_bypass': self.date_bypass.isChecked(),
            'detection_stacks': {
                'thermal': self.detect_thermal.isChecked(),
                'sar': self.detect_sar.isChecked(),
                'reverse_thermal': self.detect_reverse_thermal.isChecked(),
                'optical': self.detect_optical.isChecked(),
                'swot': self.detect_swot.isChecked(),
            },
            'location_mode': 'center_radius' if self.loc_center_radio.isChecked() else 'bbox',
            'center_lat': self.center_lat.value(),
            'center_lon': self.center_lon.value(),
            'radius_km': self.radius_km.value(),
            'bbox': {
                'w': self.bbox_w.value(),
                's': self.bbox_s.value(),
                'e': self.bbox_e.value(),
                'n': self.bbox_n.value(),
            },
            'sink_date_enabled': self.sink_date_check.isChecked(),
            'sink_date': self.sink_date.date().toString("yyyy-MM-dd"),
            'sink_days_before': self.sink_days_before.value(),
            'sink_days_after': self.sink_days_after.value(),
        }
    
    def update_queue_preview(self):
        """Update scan queue preview based on current selections."""
        params = self.get_scan_params()
        
        preview_text = []
        preview_text.append(f"Scan Mode: {params['mode']}")
        preview_text.append(f"Lakes Selected: {len(params['lakes'])}")
        for lake in params['lakes']:
            preview_text.append(f"  - {GREAT_LAKES_BBOXES[lake]['name']}")
        
        preview_text.append(f"\nScenarios Selected: {len(params['scenarios'])}")
        for scenario in params['scenarios']:
            name = SCAN_SCENARIOS[scenario]['name']
            priority = SCAN_SCENARIOS[scenario]['priority_weight']
            preview_text.append(f"  - {name} ({priority}×)")
        
        if params['start_date']:
            preview_text.append(f"\nDate Range: {params['start_date']} to {params['end_date']}")
        else:
            preview_text.append(f"\nDate Range: Auto (Low-Water Years)")
        
        # Check sensor coverage
        if params['start_date']:
            from great_lakes_scanner import check_sensor_coverage
            required_sensors = ['sentinel2a']  # Always need S2
            if self.detect_thermal.isChecked():
                required_sensors.append('landsat8')
            if self.detect_swot.isChecked():
                required_sensors.append('swot')
            
            coverage = check_sensor_coverage(
                (params['start_date'], params['end_date']),
                params['lakes'][0] if params['lakes'] else 'michigan',
                required_sensors
            )
            
            if coverage['warnings']:
                preview_text.append(f"\n⚠️ Sensor Coverage Warnings:")
                for warning in coverage['warnings']:
                    preview_text.append(f"  {warning}")
            
            preview_text.append(f"\n  Dates with full coverage: {coverage['total_dates']}")
            preview_text.append(f"  Dates missing sensors: {coverage['total_missing']}")
        
        # Check CUDA status
        from great_lakes_scanner import check_cuda_availability
        cuda_status = check_cuda_availability()
        
        preview_text.append(f"\n{'✅' if cuda_status['cuda_available'] else '⚠️'} CUDA Status:")
        if cuda_status['cuda_available']:
            preview_text.append(f"  GPU: {cuda_status['gpu_name']} (Active)")
        else:
            preview_text.append(f"  {cuda_status['warning']}")
            preview_text.append(f"  {cuda_status['accuracy_note']}")
        
        if params['mode'] == 'complete_set':
            total_scans = len(params['lakes']) * len(params['scenarios'])
            preview_text.append(f"\nTotal Scans Queued: {total_scans}")
            preview_text.append(f"Estimated Time: ~{total_scans * 15} seconds (metadata only)")
        
        self.queue_preview.setText("\n".join(preview_text))
    
    def start_scan(self):
        """Start the scan operation."""
        params = self.get_scan_params()
        
        # Validate
        if not params['lakes']:
            QMessageBox.warning(self, "No Lakes Selected", "Please select at least one lake region.")
            return
        
        if not params['scenarios']:
            QMessageBox.warning(self, "No Scenarios Selected", "Please select at least one scenario.")
            return
        
        # Start worker
        self.worker = ScanWorker(params)
        self.worker.progress.connect(self.on_progress)
        self.worker.finished.connect(self.on_finished)
        self.worker.error.connect(self.on_error)
        
        self.btn_start.setEnabled(False)
        self.progress_bar.setVisible(True)
        self.progress_bar.setRange(0, 0)  # Indeterminate
        self.status_label.setText("Scanning...")
        self.statusBar().showMessage("Scan in progress...")
        
        self.worker.start()
    
    def on_progress(self, message):
        """Handle progress updates."""
        self.status_label.setText(message)
        self.statusBar().showMessage(message)
    
    def on_finished(self, result):
        """Handle scan completion."""
        self.btn_start.setEnabled(True)
        self.progress_bar.setVisible(False)
        self.status_label.setText("Scan Complete!")
        self.statusBar().showMessage("Scan completed successfully")
        
        # Show results
        QMessageBox.information(
            self,
            "Scan Complete",
            f"Scan completed successfully!\n\n"
            f"Targets found: {len(result.get('scans', []))}\n"
            f"Check outputs/great_lakes_scans/ for KML files."
        )
        
        self.load_history()
    
    def on_error(self, error):
        """Handle scan errors."""
        self.btn_start.setEnabled(True)
        self.progress_bar.setVisible(False)
        self.status_label.setText("Error")
        self.statusBar().showMessage(f"Error: {error}")
        
        QMessageBox.critical(self, "Scan Error", f"An error occurred:\n{error}")
    
    def clear_selections(self):
        """Clear all selections."""
        for cb in self.lake_checks.values():
            cb.setChecked(False)
        for cb in self.scenario_checks.values():
            cb.setChecked(False)
        self.update_queue_preview()
    
    def load_history(self):
        """Load and display scan history."""
        init_scan_registry()
        history = get_scan_history(limit=50)
        
        if not history:
            self.history_view.setText("No scan history found.")
            return
        
        lines = ["Scan History (Last 50):\n"]
        for scan in history:
            lines.append(
                f"{scan['scan_date']} | {scan['lake_region']} | {scan['scenario']} | "
                f"Score: {scan['avg_score']:.2f} | Targets: {scan['target_count']} | "
                f"Status: {scan['storage_status']}"
            )
        
        self.history_view.setText("\n".join(lines))


# ── Main ──────────────────────────────────────────────────────────────────────

def main():
    app = QApplication(sys.argv)
    
    # Set dark theme
    app.setStyle("Fusion")
    palette = QPalette()
    palette.setColor(QPalette.Window, QColor(53, 53, 53))
    palette.setColor(QPalette.WindowText, Qt.white)
    palette.setColor(QPalette.Base, QColor(25, 25, 25))
    palette.setColor(QPalette.AlternateBase, QColor(53, 53, 53))
    palette.setColor(QPalette.ToolTipBase, Qt.white)
    palette.setColor(QPalette.ToolTipText, Qt.white)
    palette.setColor(QPalette.Text, Qt.white)
    palette.setColor(QPalette.Button, QColor(53, 53, 53))
    palette.setColor(QPalette.ButtonText, Qt.white)
    palette.setColor(QPalette.BrightText, Qt.red)
    palette.setColor(QPalette.Link, QColor(42, 130, 218))
    palette.setColor(QPalette.Highlight, QColor(42, 130, 218))
    palette.setColor(QPalette.HighlightedText, Qt.black)
    app.setPalette(palette)
    
    window = WreckHunterGUI()
    window.show()
    
    sys.exit(app.exec_())


if __name__ == '__main__':
    main()
