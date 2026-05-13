import subprocess
import json
import logging

class SarSlickOrchestrator:
    def __init__(self, rust_binary_path="./cesarops-slicer/target/release/sar_slick_worker"):
        self.cmd = rust_binary_path
        logging.basicConfig(level=logging.INFO)

    def dispatch_sar_scan(self, min_lon, min_lat, max_lon, max_lat, output_json="sar_results.json"):
        logging.info(f"Orchestrating SAR Tier 1 Specialist against box: {min_lon}, {min_lat}, {max_lon}, {max_lat}")
        try:
            # We call the compiled Rust code via Popen to delegate raster computations 
            result = subprocess.run([
                "cargo", "run", "--release", "--bin", "sar_slick_worker", "--",
                "--bbox", f"{min_lon},{min_lat},{max_lon},{max_lat}",
                "--output", output_json
            ], cwd="cesarops-slicer", capture_output=True, text=True)
            
            if result.returncode != 0:
                logging.error(f"Worker failed: {result.stderr}")
                return None
            
            logging.info(f"Rust subprocess out: {result.stdout.strip()}")
            with open(f"cesarops-slicer/{output_json}", "r", encoding="utf-8") as f:
                return json.load(f)
                
        except Exception as e:
            logging.error(f"Failed to communicate with Specialist: {e}")
            return None

if __name__ == "__main__":
    orch = SarSlickOrchestrator()
    # Test Erie coordinate bounding box
    results = orch.dispatch_sar_scan(-83.0, 41.5, -82.8, 41.7, "erie_test.json")
    print(f"ANOMALY PAYLOAD: {results}")