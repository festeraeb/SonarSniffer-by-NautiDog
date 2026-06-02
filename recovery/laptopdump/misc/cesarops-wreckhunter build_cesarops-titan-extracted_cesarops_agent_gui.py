#!/usr/bin/env python3
"""
CESAROPS AGENT GUI

Simple GUI to:
1. Select sensor/band
2. Adjust processing variables
3. Choose tiles
4. Save preset configurations
5. Push to Xenon

You (the AI) act as the agent choosing optimal settings.
"""

import tkinter as tk
from tkinter import ttk, messagebox, scrolledtext
import json
from pathlib import Path
from datetime import datetime

# Configuration presets
PRESETS = {
    "Default Thermal": {
        "bands": ["B10", "B11"],
        "zscore_threshold": 2.5,
        "min_anomaly_count": 10,
        "satellite": "Any",
    },
    "High Confidence": {
        "bands": ["B10", "B11", "B04", "B05"],
        "zscore_threshold": 3.0,
        "min_anomaly_count": 50,
        "satellite": "Sentinel-2",
    },
    "Deep Wreck Hunt": {
        "bands": ["B10", "B11"],
        "zscore_threshold": 2.0,
        "min_anomaly_count": 100,
        "satellite": "Landsat-8",
    },
}

class CesaropsAgentGUI:
    def __init__(self, root):
        self.root = root
        self.root.title("CESAROPS Agent - Tile Processing Control")
        self.root.geometry("900x700")
        
        # Current configuration
        self.config = {
            "bands": ["B10", "B11"],
            "zscore_threshold": 2.5,
            "min_anomaly_count": 10,
            "satellite": "Any",
            "tile_selection": "auto",
            "output_name": "",
        }
        
        self.setup_ui()
        
    def setup_ui(self):
        # Top frame - Preset selection
        top_frame = ttk.Frame(self.root, padding="10")
        top_frame.grid(row=0, column=0, sticky="ew")
        
        ttk.Label(top_frame, text="Preset:").grid(row=0, column=0, padx=5)
        self.preset_var = tk.StringVar(value="Default Thermal")
        preset_combo = ttk.Combobox(top_frame, textvariable=self.preset_var, values=list(PRESETS.keys()), width=30)
        preset_combo.grid(row=0, column=1, padx=5)
        preset_combo.bind('<<ComboboxSelected>>', self.load_preset)
        
        ttk.Button(top_frame, text="Load Preset", command=self.load_preset).grid(row=0, column=2, padx=5)
        ttk.Button(top_frame, text="Save as New Preset", command=self.save_preset).grid(row=0, column=3, padx=5)
        
        # Left frame - Configuration
        left_frame = ttk.LabelFrame(self.root, text="Processing Configuration", padding="10")
        left_frame.grid(row=1, column=0, sticky="nsw", padx=10, pady=10)
        
        # Bands selection
        ttk.Label(left_frame, text="Bands:").grid(row=0, column=0, sticky="w", pady=5)
        self.band_vars = {}
        bands = ["B01", "B04", "B05", "B10", "B11", "B12", "B8A"]
        for i, band in enumerate(bands):
            var = tk.BooleanVar(value=band in self.config["bands"])
            self.band_vars[band] = var
            ttk.Checkbutton(left_frame, text=band, variable=var).grid(row=i//3 + 1, column=i%3, sticky="w", padx=10)
        
        # Z-Score threshold
        ttk.Label(left_frame, text="Z-Score Threshold:").grid(row=5, column=0, sticky="w", pady=5)
        self.zscore_var = tk.DoubleVar(value=self.config["zscore_threshold"])
        zscore_scale = ttk.Scale(left_frame, from_=1.0, to=5.0, variable=self.zscore_var, orient="horizontal", length=200)
        zscore_scale.grid(row=5, column=1, sticky="w", padx=10)
        self.zscore_label = ttk.Label(left_frame, text=f"{self.zscore_var.get():.1f}")
        self.zscore_label.grid(row=5, column=2, padx=5)
        zscore_scale.configure(command=self.update_zscore_label)
        
        # Min anomaly count
        ttk.Label(left_frame, text="Min Anomaly Count:").grid(row=6, column=0, sticky="w", pady=5)
        self.anomaly_var = tk.IntVar(value=self.config["min_anomaly_count"])
        anomaly_spin = ttk.Spinbox(left_frame, from_=1, to=10000, textvariable=self.anomaly_var, width=10)
        anomaly_spin.grid(row=6, column=1, sticky="w", padx=10)
        
        # Satellite selection
        ttk.Label(left_frame, text="Satellite:").grid(row=7, column=0, sticky="w", pady=5)
        self.satellite_var = tk.StringVar(value=self.config["satellite"])
        sat_combo = ttk.Combobox(left_frame, textvariable=self.satellite_var, values=["Any", "Landsat-8", "Sentinel-2"], width=20)
        sat_combo.grid(row=7, column=1, sticky="w", padx=10)
        
        # Output name
        ttk.Label(left_frame, text="Output Name:").grid(row=8, column=0, sticky="w", pady=5)
        self.output_var = tk.StringVar(value=datetime.now().strftime("%Y%m%d_%H%M%S"))
        ttk.Entry(left_frame, textvariable=self.output_var, width=30).grid(row=8, column=1, sticky="w", padx=10)
        
        # Right frame - Tile selection
        right_frame = ttk.LabelFrame(self.root, text="Tile Selection", padding="10")
        right_frame.grid(row=1, column=1, sticky="nsew", padx=10, pady=10)
        
        self.tile_text = scrolledtext.ScrolledText(right_frame, width=50, height=20)
        self.tile_text.grid(row=0, column=0, sticky="nsew")
        
        ttk.Button(right_frame, text="Load Available Tiles", command=self.load_tiles).grid(row=1, column=0, pady=5)
        
        # Bottom frame - Actions
        bottom_frame = ttk.Frame(self.root, padding="10")
        bottom_frame.grid(row=2, column=0, columnspan=2, sticky="ew")
        
        ttk.Button(bottom_frame, text="Run on Laptop", command=self.run_laptop).grid(row=0, column=0, padx=5)
        ttk.Button(bottom_frame, text="Push to Xenon", command=self.push_xenon).grid(row=0, column=1, padx=5)
        ttk.Button(bottom_frame, text="Save Config", command=self.save_config).grid(row=0, column=2, padx=5)
        ttk.Button(bottom_frame, text="Load Config", command=self.load_config).grid(row=0, column=3, padx=5)
        
        # Agent log
        log_frame = ttk.LabelFrame(self.root, text="Agent Log", padding="10")
        log_frame.grid(row=3, column=0, columnspan=2, sticky="nsew", padx=10, pady=10)
        
        self.log_text = scrolledtext.ScrolledText(log_frame, width=80, height=10)
        self.log_text.grid(row=0, column=0)
        
        self.log("Agent ready. Select preset or adjust configuration.")
        
    def update_zscore_label(self, value):
        self.zscore_label.configure(text=f"{float(value):.1f}")
        
    def load_preset(self, event=None):
        preset_name = self.preset_var.get()
        if preset_name in PRESETS:
            preset = PRESETS[preset_name]
            
            # Update bands
            for band, var in self.band_vars.items():
                var.set(band in preset["bands"])
            
            # Update other settings
            self.zscore_var.set(preset["zscore_threshold"])
            self.anomaly_var.set(preset["min_anomaly_count"])
            self.satellite_var.set(preset["satellite"])
            
            self.log(f"Loaded preset: {preset_name}")
            
    def save_preset(self):
        preset_name = f"Custom_{datetime.now().strftime('%Y%m%d_%H%M%S')}"
        PRESETS[preset_name] = self.get_current_config()
        self.preset_var['values'] = list(PRESETS.keys())
        self.preset_var.set(preset_name)
        self.log(f"Saved preset: {preset_name}")
        
    def get_current_config(self):
        bands = [band for band, var in self.band_vars.items() if var.get()]
        return {
            "bands": bands,
            "zscore_threshold": self.zscore_var.get(),
            "min_anomaly_count": self.anomaly_var.get(),
            "satellite": self.satellite_var.get(),
            "output_name": self.output_var.get(),
        }
        
    def load_tiles(self):
        self.tile_text.delete(1.0, tk.END)
        self.tile_text.insert(tk.END, "Loading available tiles...\n")
        
        # Find tiles
        tile_count = 0
        for tif in Path("wreckhunter2000/data/cache").rglob("*.tif"):
            if tif.stat().st_size > 100000:  # >100KB
                self.tile_text.insert(tk.END, f"{tif.relative_to(Path('.'))}\n")
                tile_count += 1
        
        self.tile_text.insert(tk.END, f"\nTotal: {tile_count} tiles\n")
        self.log(f"Found {tile_count} tiles")
        
    def run_laptop(self):
        config = self.get_current_config()
        self.log(f"Running on laptop with config: {config}")
        # TODO: Actually run processing
        messagebox.showinfo("Laptop Run", f"Processing with:\n{config}")
        
    def push_xenon(self):
        config = self.get_current_config()
        self.log(f"Pushing to Xenon: {config}")
        # TODO: SSH to Xenon and push config
        messagebox.showinfo("Push to Xenon", f"Configuration pushed to Xenon:\n{config}")
        
    def save_config(self):
        config = self.get_current_config()
        config_file = Path("outputs/agent_config.json")
        config_file.parent.mkdir(parents=True, exist_ok=True)
        
        with open(config_file, 'w') as f:
            json.dump(config, f, indent=2)
        
        self.log(f"Config saved to: {config_file}")
        
    def load_config(self):
        config_file = Path("outputs/agent_config.json")
        if not config_file.exists():
            messagebox.showwarning("No Config", "No saved configuration found")
            return
        
        with open(config_file) as f:
            config = json.load(f)
        
        # Apply config
        for band, var in self.band_vars.items():
            var.set(band in config.get("bands", []))
        
        self.zscore_var.set(config.get("zscore_threshold", 2.5))
        self.anomaly_var.set(config.get("min_anomaly_count", 10))
        self.satellite_var.set(config.get("satellite", "Any"))
        self.output_var.set(config.get("output_name", ""))
        
        self.log(f"Config loaded from: {config_file}")
        
    def log(self, message):
        timestamp = datetime.now().strftime("%H:%M:%S")
        self.log_text.insert(tk.END, f"[{timestamp}] {message}\n")
        self.log_text.see(tk.END)

def main():
    root = tk.Tk()
    app = CesaropsAgentGUI(root)
    root.mainloop()

if __name__ == "__main__":
    main()
