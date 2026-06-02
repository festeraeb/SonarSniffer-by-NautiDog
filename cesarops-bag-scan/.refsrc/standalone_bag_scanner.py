#!/usr/bin/env python3
"""
Standalone BAG File Scanner with Anomaly Height Calculation
Processes BAG files independently - PDF data optional for verification only
"""

import os
import sys
import numpy as np
import json
from pathlib import Path
from datetime import datetime
import math
from typing import Dict, List, Tuple, Optional
import warnings

warnings.filterwarnings("ignore")

# Try to import required libraries
try:
    import rasterio

    RASTERIO_AVAILABLE = True
except ImportError:
    RASTERIO_AVAILABLE = False
    print("⚠️ rasterio not available - install with: pip install rasterio")

try:
    from scipy import ndimage

    SCIPY_AVAILABLE = True
except ImportError:
    SCIPY_AVAILABLE = False
    print("⚠️ scipy not available - install with: pip install scipy")

try:
    from pyproj import Transformer

    PROJ_AVAILABLE = True
except ImportError:
    PROJ_AVAILABLE = False
    print("⚠️ pyproj not available - coordinate conversion limited")


class StandaloneBagScanner:
    """
    Standalone BAG file scanner that calculates anomaly heights
    Works independently from PDF data - PDF cross-referencing is optional
    """

    def __init__(self, config: Dict = None):
        self.config = config or {}

        # Detection parameters
        self.min_anomaly_height_m = self.config.get(
            "min_anomaly_height_m", 0.5
        )  # 0.5m minimum
        self.max_anomaly_height_m = self.config.get(
            "max_anomaly_height_m", 50.0
        )  # 50m maximum
        self.min_size_sq_meters = self.config.get("min_size_sq_meters", 10.0)
        self.local_window_size = self.config.get(
            "local_window_size", 100
        )  # pixels for local baseline

        # Conversion factors
        self.feet_per_meter = 3.28084

        # Coordinate transformer
        self.transformers = {}

        # Output directory
        self.output_dir = Path(self.config.get("output_dir", "bag_scan_results"))
        self.output_dir.mkdir(exist_ok=True)

        # Optional PDF verification data (loaded separately)
        self.pdf_coordinates = []
        self.verification_radius_m = 100  # Match within 100m

        print("🔬 Standalone BAG Scanner initialized")
        print(f"   Min anomaly height: {self.min_anomaly_height_m}m")
        print(f"   Local window size: {self.local_window_size} pixels")

    def estimate_dimensions(self, size_sq_feet: float) -> Tuple[float, float]:
        """Estimate length and width from square footage using shipwreck aspect ratios"""
        if size_sq_feet < 100:
            side = math.sqrt(size_sq_feet)
            return side, side
        elif size_sq_feet < 1000:
            width = math.sqrt(size_sq_feet / 2)
            length = width * 2
            return length, width
        elif size_sq_feet < 10000:
            width = math.sqrt(size_sq_feet / 3)
            length = width * 3
            return length, width
        else:
            width = math.sqrt(size_sq_feet / 4)
            length = width * 4
            return length, width

    def get_transformer(self, crs_string: str):
        """Get or create a coordinate transformer"""
        if crs_string not in self.transformers and PROJ_AVAILABLE:
            try:
                self.transformers[crs_string] = Transformer.from_crs(
                    crs_string, "EPSG:4326", always_xy=True
                )
            except:
                self.transformers[crs_string] = None
        return self.transformers.get(crs_string)

    def convert_to_latlon(
        self, x: float, y: float, crs_string: str
    ) -> Tuple[float, float]:
        """Convert coordinates to lat/lon"""
        transformer = self.get_transformer(crs_string)
        if transformer:
            try:
                lon, lat = transformer.transform(x, y)
                return lat, lon
            except:
                pass
        # Return as-is if no transformation available
        return y, x

    def calculate_local_baseline(
        self, elevation: np.ndarray, row: int, col: int, mask: np.ndarray
    ) -> Tuple[float, float]:
        """
        Calculate local seabed baseline around a point
        Returns (baseline_depth, baseline_std)
        """
        half_window = self.local_window_size // 2

        # Get window bounds
        row_min = max(0, row - half_window)
        row_max = min(elevation.shape[0], row + half_window)
        col_min = max(0, col - half_window)
        col_max = min(elevation.shape[1], col + half_window)

        # Extract window
        window = elevation[row_min:row_max, col_min:col_max]
        window_mask = mask[row_min:row_max, col_min:col_max]

        # Get valid values (excluding the anomaly itself)
        valid_values = window[window_mask & ~np.isnan(window)]

        if len(valid_values) < 10:
            return np.nan, np.nan

        # Use median for robust baseline (less affected by outliers)
        baseline = np.median(valid_values)
        baseline_std = np.std(valid_values)

        return baseline, baseline_std

    def detect_anomalies_with_height(
        self, elevation: np.ndarray, transform, crs_string: str
    ) -> List[Dict]:
        """
        Detect anomalies and calculate their heights above the seabed
        """
        detections = []

        # Create valid data mask
        valid_mask = ~np.isnan(elevation) & (elevation > -1000) & (elevation < 1000)

        if not valid_mask.any():
            return detections

        # Calculate global statistics for initial filtering
        valid_data = elevation[valid_mask]
        global_mean = np.nanmean(valid_data)
        global_std = np.nanstd(valid_data)

        # Initial anomaly detection (find areas that deviate from surroundings)
        # For bathymetry, anomalies are typically shallower (higher elevation values = less depth)
        anomaly_threshold = 2.0 * global_std

        # Detect both positive and negative anomalies
        anomaly_mask = np.abs(elevation - global_mean) > anomaly_threshold
        anomaly_mask = anomaly_mask & valid_mask

        if not anomaly_mask.any():
            return detections

        # Label connected components
        labeled, num_features = ndimage.label(anomaly_mask)

        pixel_size_x = abs(transform.a)
        pixel_size_y = abs(transform.e)
        pixel_area_m2 = pixel_size_x * pixel_size_y

        for label_id in range(1, num_features + 1):
            component_mask = labeled == label_id
            component_size = np.sum(component_mask)

            # Size filter
            size_sq_meters = component_size * pixel_area_m2
            if size_sq_meters < self.min_size_sq_meters:
                continue

            # Get component coordinates
            rows, cols = np.where(component_mask)
            center_row = int(np.mean(rows))
            center_col = int(np.mean(cols))

            # Get elevation values within the anomaly
            anomaly_elevations = elevation[component_mask]
            anomaly_elevations = anomaly_elevations[~np.isnan(anomaly_elevations)]

            if len(anomaly_elevations) == 0:
                continue

            # Calculate local baseline (surrounding seabed depth)
            # Exclude the anomaly from baseline calculation
            baseline_mask = valid_mask.copy()
            baseline_mask[component_mask] = False

            baseline_depth, baseline_std = self.calculate_local_baseline(
                elevation, center_row, center_col, baseline_mask
            )

            if np.isnan(baseline_depth):
                continue

            # Calculate anomaly height
            # Shallowest point of anomaly (highest elevation = least depth)
            shallowest_point = np.max(
                anomaly_elevations
            )  # Highest elevation = shallowest
            deepest_point = np.min(anomaly_elevations)
            mean_anomaly_depth = np.mean(anomaly_elevations)

            # Anomaly height = baseline depth - shallowest point of anomaly
            # In bathymetry: more negative = deeper, so height = baseline - shallowest
            anomaly_height_m = abs(baseline_depth - shallowest_point)

            # Also calculate "relief" - total vertical extent of anomaly
            anomaly_relief_m = abs(shallowest_point - deepest_point)

            # Filter by height
            if anomaly_height_m < self.min_anomaly_height_m:
                continue
            if anomaly_height_m > self.max_anomaly_height_m:
                continue

            # Convert to coordinates
            easting, northing = transform * (center_col, center_row)
            lat, lon = self.convert_to_latlon(easting, northing, crs_string)

            # Calculate bounding box
            min_row, max_row = np.min(rows), np.max(rows)
            min_col, max_col = np.min(cols), np.max(cols)

            # Dimensions in meters
            length_m = (max_row - min_row + 1) * pixel_size_y
            width_m = (max_col - min_col + 1) * pixel_size_x

            # Convert to feet
            size_sq_feet = size_sq_meters * (self.feet_per_meter**2)
            length_ft = length_m * self.feet_per_meter
            width_ft = width_m * self.feet_per_meter
            anomaly_height_ft = anomaly_height_m * self.feet_per_meter

            # Calculate confidence based on multiple factors
            height_confidence = min(
                1.0, anomaly_height_m / 3.0
            )  # Higher anomalies = more confident
            size_confidence = min(
                1.0, size_sq_meters / 500.0
            )  # Larger = more confident
            std_confidence = (
                1.0 - min(1.0, baseline_std / anomaly_height_m)
                if anomaly_height_m > 0
                else 0
            )

            confidence = (height_confidence + size_confidence + std_confidence) / 3.0

            # Determine if this is likely a wreck vs natural feature
            aspect_ratio = max(length_m, width_m) / max(min(length_m, width_m), 1)
            is_elongated = aspect_ratio > 2.0  # Ship-like if elongated

            detection = {
                "latitude": float(lat),
                "longitude": float(lon),
                "easting": float(easting),
                "northing": float(northing),
                # Size measurements
                "size_sq_meters": float(round(size_sq_meters, 2)),
                "size_sq_feet": float(round(size_sq_feet, 2)),
                "length_meters": float(round(length_m, 2)),
                "width_meters": float(round(width_m, 2)),
                "length_feet": float(round(length_ft, 1)),
                "width_feet": float(round(width_ft, 1)),
                "aspect_ratio": float(round(aspect_ratio, 2)),
                # HEIGHT MEASUREMENTS (key new data)
                "anomaly_height_meters": float(round(anomaly_height_m, 2)),
                "anomaly_height_feet": float(round(anomaly_height_ft, 1)),
                "anomaly_relief_meters": float(round(anomaly_relief_m, 2)),
                # Depth context
                "seabed_depth_meters": float(round(abs(baseline_depth), 2)),
                "least_depth_meters": float(round(abs(shallowest_point), 2)),
                "least_depth_feet": float(round(
                    abs(shallowest_point) * self.feet_per_meter, 1
                )),
                # Internal info for plotting
                "bbox_pixels": [int(min_row), int(max_row), int(min_col), int(max_col)],
                # Classification hints
                "confidence": float(round(confidence, 3)),
                "is_elongated": bool(is_elongated),
                "likely_type": (
                    "wreck_candidate"
                    if is_elongated and anomaly_height_m > 1.0
                    else "anomaly"
                ),
                # Method info
                "method": "standalone_height_detection",
            }

            detections.append(detection)

        return detections

    def scan_bag_file(self, bag_path: str) -> Dict:
        """Scan a single BAG file for anomalies with height calculation"""

        if not RASTERIO_AVAILABLE:
            return {"error": "rasterio not available", "file": bag_path}

        bag_path = Path(bag_path)
        print(f"\n🔬 Scanning: {bag_path.name}")

        try:
            with rasterio.open(bag_path) as src:
                elevation = src.read(1)
                transform = src.transform
                crs = str(src.crs) if src.crs else "EPSG:4326"

                print(f"   📐 Size: {elevation.shape[1]} x {elevation.shape[0]} pixels")
                print(f"   🗺️  CRS: {crs}")

                # Detect anomalies with heights
                detections = self.detect_anomalies_with_height(
                    elevation, transform, crs
                )

                # Sort by confidence
                detections.sort(key=lambda x: x["confidence"], reverse=True)

                # Summary statistics
                if detections:
                    heights = [d["anomaly_height_meters"] for d in detections]
                    print(f"   ✅ Found {len(detections)} anomalies")
                    print(
                        f"   📏 Height range: {min(heights):.1f}m - {max(heights):.1f}m"
                    )
                    print(
                        f"   🚢 Wreck candidates: {sum(1 for d in detections if d['likely_type'] == 'wreck_candidate')}"
                    )
                    
                    if self.config.get("plot_visuals", False):
                        out_dir = self.config.get("output_dir", bag_path.parent)
                        os.makedirs(out_dir, exist_ok=True)
                        try:
                            import plotly.graph_objects as go
                            print("   🎨 Generating 3D proportional visualizations...")
                            
                            for idx, det in enumerate(detections):
                                min_r, max_r, min_c, max_c = det.get("bbox_pixels", [0,0,0,0])
                                if max_r <= min_r or max_c <= min_c:
                                    continue
                                    
                                # Calculate generous padding to see lakebed context
                                h_pad = max(50, int((max_r - min_r) * 1.5))
                                w_pad = max(50, int((max_c - min_c) * 1.5))
                                h_pad = min(h_pad, 500)
                                w_pad = min(w_pad, 500)
                                
                                r_start = max(0, min_r - h_pad)
                                r_end = min(elevation.shape[0], max_r + h_pad)
                                c_start = max(0, min_c - w_pad)
                                c_end = min(elevation.shape[1], max_c + w_pad)
                                
                                elev_crop = elevation[r_start:r_end, c_start:c_end]
                                
                                # Downsample massive meshes to ~200x200
                                stride_y = max(1, (r_end - r_start) // 200)
                                stride_x = max(1, (c_end - c_start) // 200)
                                
                                elev_crop = elev_crop[::stride_y, ::stride_x]
                                
                                c_grid, r_grid = np.meshgrid(
                                    np.arange(c_start, c_end, stride_x),
                                    np.arange(r_start, r_end, stride_y)
                                )
                                # Convert pixel indices to Easting/Northing in meters
                                x_proj, y_proj = transform * (c_grid, r_grid)
                                
                                # Convert EVERYTHING to feet for uniform 1:1:1 aspect ratio
                                x_ft = x_proj * 3.28084
                                y_ft = y_proj * 3.28084
                                z_ft = elev_crop * 3.28084
                                
                                fig = go.Figure()
                                
                                # Topographical surface
                                fig.add_trace(go.Surface(
                                    x=x_ft, y=y_ft, z=z_ft,
                                    colorscale='Viridis',
                                    colorbar_title='Depth (ft)',
                                    contours={
                                        "z": {"show": True, "start": np.nanmin(z_ft), "end": np.nanmax(z_ft), "size": 2.0, "color": "white"}
                                    }
                                ))
                                
                                fig.update_layout(
                                    title=f"Protrusion Target [{det.get('likely_type', 'anomaly').upper()}]<br>Height: {det.get('anomaly_height_feet')}ft, Size: {det.get('length_feet',0):.0f}L x {det.get('width_feet',0):.0f}W",
                                    scene=dict(
                                        xaxis_title='Easting (ft)',
                                        yaxis_title='Northing (ft)',
                                        zaxis_title='Depth (ft)',
                                        aspectmode='data' # CRITICAL: Forces 1 unit in X/Y to equal 1 unit in Z
                                    ),
                                    margin=dict(l=0, r=0, b=0, t=60)
                                )
                                
                                out_html = os.path.join(out_dir, f"{bag_path.stem}_{idx}_protrusion.html")
                                fig.write_html(out_html)
                                
                        except ImportError:
                            print("   ⚠️ Plotly not installed, skipping visuals.")
                        except Exception as e:
                            print(f"   ❌ Failed to generate visual: {e}")

                else:
                    print(f"   ⚠️  No significant anomalies found")

                return {
                    "file": bag_path.name,
                    "path": str(bag_path),
                    "crs": crs,
                    "shape": list(elevation.shape),
                    "total_detections": len(detections),
                    "wreck_candidates": sum(
                        1 for d in detections if d["likely_type"] == "wreck_candidate"
                    ),
                    "detections": detections,
                    "scan_time": datetime.now().isoformat(),
                }

        except Exception as e:
            print(f"   ❌ Error: {e}")
            return {
                "file": bag_path.name,
                "error": str(e),
                "scan_time": datetime.now().isoformat(),
            }

    def load_pdf_verification_data(self, pdf_data_path: str = None):
        """
        Optional: Load PDF-extracted coordinates for verification
        This does NOT change detection - only used for cross-referencing
        """
        if pdf_data_path is None:
            # Try to find existing PDF data
            possible_paths = [
                "pdf_vulnerability_scan_results.json",
                "development_and_tools/redaction_breaker_results_*.json",
            ]
            for pattern in possible_paths:
                matches = list(Path(".").glob(pattern))
                if matches:
                    pdf_data_path = str(matches[0])
                    break

        if pdf_data_path and Path(pdf_data_path).exists():
            try:
                with open(pdf_data_path, "r") as f:
                    data = json.load(f)
                # Extract coordinates from PDF data
                # This would need to be adapted based on actual PDF data structure
                print(f"📄 Loaded PDF verification data from {pdf_data_path}")
            except Exception as e:
                print(f"⚠️  Could not load PDF data: {e}")

    def verify_against_pdf(self, detections: List[Dict]) -> List[Dict]:
        """
        Cross-reference BAG detections against PDF-extracted coordinates
        Returns detections with verification status
        """
        if not self.pdf_coordinates:
            return detections

        for detection in detections:
            detection["pdf_verified"] = False
            detection["pdf_match_distance_m"] = None

            det_lat = detection["latitude"]
            det_lon = detection["longitude"]

            for pdf_coord in self.pdf_coordinates:
                pdf_lat = pdf_coord.get("latitude")
                pdf_lon = pdf_coord.get("longitude")

                if pdf_lat and pdf_lon:
                    # Calculate distance
                    dist_m = self._haversine_distance(
                        det_lat, det_lon, pdf_lat, pdf_lon
                    )

                    if dist_m <= self.verification_radius_m:
                        detection["pdf_verified"] = True
                        detection["pdf_match_distance_m"] = round(dist_m, 1)
                        detection["pdf_match_name"] = pdf_coord.get("name", "Unknown")
                        break

        return detections

    def _haversine_distance(
        self, lat1: float, lon1: float, lat2: float, lon2: float
    ) -> float:
        """Calculate distance between two points in meters"""
        R = 6371000  # Earth's radius in meters

        lat1_rad = math.radians(lat1)
        lat2_rad = math.radians(lat2)
        delta_lat = math.radians(lat2 - lat1)
        delta_lon = math.radians(lon2 - lon1)

        a = (
            math.sin(delta_lat / 2) ** 2
            + math.cos(lat1_rad) * math.cos(lat2_rad) * math.sin(delta_lon / 2) ** 2
        )
        c = 2 * math.atan2(math.sqrt(a), math.sqrt(1 - a))

        return R * c

    def scan_directory(self, directory: str, pattern: str = "*.bag") -> Dict:
        """Scan all BAG files in a directory"""

        directory = Path(directory)
        if not directory.exists():
            return {"error": f"Directory not found: {directory}"}

        bag_files = list(directory.glob(pattern))
        print(f"\n🏞️  STANDALONE BAG FILE SCANNER")
        print("=" * 50)
        print(f"📁 Directory: {directory}")
        print(f"🎯 Found {len(bag_files)} BAG files")

        all_results = []
        total_detections = 0
        total_wreck_candidates = 0

        for bag_file in bag_files:
            result = self.scan_bag_file(bag_file)
            all_results.append(result)

            if "detections" in result:
                total_detections += result["total_detections"]
                total_wreck_candidates += result["wreck_candidates"]

        # Create summary
        timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")

        summary = {
            "scan_info": {
                "timestamp": timestamp,
                "directory": str(directory),
                "files_scanned": len(bag_files),
                "total_detections": total_detections,
                "total_wreck_candidates": total_wreck_candidates,
                "min_anomaly_height_m": self.min_anomaly_height_m,
                "local_window_size": self.local_window_size,
            },
            "results": all_results,
        }

        # Save results
        json_file = self.output_dir / f"bag_scan_{timestamp}.json"
        with open(json_file, "w") as f:
            json.dump(summary, f, indent=2)

        # Create human-readable report
        txt_file = self.output_dir / f"bag_scan_{timestamp}.txt"
        self._write_text_report(summary, txt_file)

        print(f"\n✅ Scan Complete!")
        print(f"📊 Total detections: {total_detections}")
        print(f"🚢 Wreck candidates: {total_wreck_candidates}")
        print(f"📁 Results: {json_file}")

        return summary

    def _write_text_report(self, summary: Dict, output_path: Path):
        """Write human-readable text report with anomaly heights"""

        with open(output_path, "w", encoding="utf-8") as f:
            f.write("STANDALONE BAG FILE SCAN RESULTS\n")
            f.write("=" * 60 + "\n\n")

            info = summary["scan_info"]
            f.write(f"Scan Date: {info['timestamp']}\n")
            f.write(f"Directory: {info['directory']}\n")
            f.write(f"Files Scanned: {info['files_scanned']}\n")
            f.write(f"Total Detections: {info['total_detections']}\n")
            f.write(f"Wreck Candidates: {info['total_wreck_candidates']}\n")
            f.write(f"Min Anomaly Height: {info['min_anomaly_height_m']}m\n\n")

            f.write("DETECTIONS BY FILE:\n")
            f.write("-" * 60 + "\n\n")

            for result in summary["results"]:
                f.write(f"\n📁 {result['file']}:\n")

                if "error" in result:
                    f.write(f"   ❌ Error: {result['error']}\n")
                    continue

                f.write(f"   Detections: {result['total_detections']}\n")
                f.write(f"   Wreck Candidates: {result['wreck_candidates']}\n\n")

                if result["detections"]:
                    # Header
                    f.write(
                        "   {:^10} {:^11} | {:^8} {:^8} | {:^10} {:^10} | {:^6}\n".format(
                            "Latitude",
                            "Longitude",
                            "Height",
                            "Depth",
                            "Length",
                            "Width",
                            "Conf",
                        )
                    )
                    f.write("   " + "-" * 75 + "\n")

                    for det in result["detections"][:25]:  # Top 25 per file
                        f.write(
                            "   {:10.6f} {:11.6f} | {:6.1f}m {:6.1f}m | {:7.1f}ft {:7.1f}ft | {:.2f}\n".format(
                                det["latitude"],
                                det["longitude"],
                                det["anomaly_height_meters"],
                                det["least_depth_meters"],
                                det["length_feet"],
                                det["width_feet"],
                                det["confidence"],
                            )
                        )

                    if len(result["detections"]) > 25:
                        f.write(
                            f"   ... and {len(result['detections']) - 25} more detections\n"
                        )


def main():
    """Main entry point for standalone BAG scanning"""
    import argparse

    parser = argparse.ArgumentParser(
        description="Standalone BAG File Scanner with Height Detection"
    )
    parser.add_argument(
        "path", nargs="?", default=".", help="BAG file or directory to scan"
    )
    parser.add_argument(
        "--min-height", type=float, default=0.5, help="Minimum anomaly height in meters"
    )
    parser.add_argument(
        "--output", type=str, default="bag_scan_results", help="Output directory"
    )
    parser.add_argument(
        "--pdf-verify", type=str, help="Optional PDF data for verification"
    )

    args = parser.parse_args()

    config = {"min_anomaly_height_m": args.min_height, "output_dir": args.output}

    scanner = StandaloneBagScanner(config)

    # Load PDF verification data if provided
    if args.pdf_verify:
        scanner.load_pdf_verification_data(args.pdf_verify)

    path = Path(args.path)

    if path.is_file() and path.suffix.lower() == ".bag":
        # Single file
        result = scanner.scan_bag_file(path)
        print(json.dumps(result, indent=2))
    elif path.is_dir():
        # Directory scan
        scanner.scan_directory(path)
    else:
        # Try common locations
        common_dirs = ["Lake Erie Bag Files", "Michigan_BAG_Files_2015_Present", "."]
        for dir_name in common_dirs:
            if Path(dir_name).exists():
                scanner.scan_directory(dir_name)
                break


if __name__ == "__main__":
    main()
