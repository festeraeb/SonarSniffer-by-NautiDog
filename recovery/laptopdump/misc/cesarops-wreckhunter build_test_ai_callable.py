"""
test_ai_callable.py - Demonstrate AI-callable tool workflow

Shows how an AI agent can adjust parameters and call tools dynamically.
"""

import sys
from pathlib import Path

# Add parent to path
sys.path.insert(0, str(Path(__file__).parent))

print("="*70)
print("AI-CALLABLE TOOL DEMONSTRATION")
print("="*70)
print()

# =============================================================================
# SCENARIO 1: AI adjusts for Lake Superior (deep, cold water)
# =============================================================================

print("SCENARIO 1: AI searching for Andaste in Lake Superior")
print("-"*70)

from global_controls import GlobalScannerSettings

settings = GlobalScannerSettings()
settings.update_for_lake('superior')
settings.update_for_target('andaste')

print(f"AI Configuration:")
print(f"  Lake: Superior (deep, cold)")
print(f"  Target: Andaste (310ft whaleback)")
print(f"  Sensitivity: {settings.sensitivity} (aggressive)")
print(f"  Curvelet scales: {settings.curvelet_scales}, angles: {settings.curvelet_angles}")
print()

# Import curvelet library with AI settings
try:
    from wreckhunter2000.gpu_curvelets import CurveletSettings, apply_curvelet
    
    curvelet_settings = CurveletSettings.from_global(settings)
    print(f"Curvelet library loaded with AI settings:")
    print(f"  {curvelet_settings.to_dict()}")
    print()
except ImportError as e:
    print(f"Note: PyTorch not installed (install with: pip install torch)")
    print(f"  Curvelet tools available when PyTorch CUDA is installed")
    print()

# =============================================================================
# SCENARIO 2: AI adjusts for workboat in Lake Erie (shallow, noisy)
# =============================================================================

print("SCENARIO 2: AI searching for Bridge Builder X in Lake Erie")
print("-"*70)

settings2 = GlobalScannerSettings()
settings2.update_for_lake('erie')
settings2.update_for_target('workboat')

print(f"AI Configuration:")
print(f"  Lake: Erie (shallow, noisy)")
print(f"  Target: Workboat (50-150ft)")
print(f"  Sensitivity: {settings2.sensitivity} (conservative)")
print(f"  Curvelet scales: {settings2.curvelet_scales}, angles: {settings2.curvelet_angles}")
print()

# =============================================================================
# SCENARIO 3: AI adjusts for monster mass detection
# =============================================================================

print("SCENARIO 3: AI searching for 14000+ ton monster mass")
print("-"*70)

settings3 = GlobalScannerSettings()
settings3.update_for_target('monster')

print(f"AI Configuration:")
print(f"  Target: Monster (14000+ tons)")
print(f"  Sensitivity: {settings3.sensitivity} (very aggressive)")
print()

# =============================================================================
# SCENARIO 4: AI calls detection tool with custom parameters
# =============================================================================

print("SCENARIO 4: AI calls detection tool with custom parameters")
print("-"*70)

try:
    from wreckhunter2000.scripts.huron_detect_and_fuse import DetectionSettings
    
    # AI creates custom settings for specific conditions
    ai_settings = DetectionSettings(
        sensitivity=1.5,      # Very sensitive for deep targets
        min_size=5,           # Small minimum size
        fuse_distance_m=100   # Large fusion distance
    )
    
    print(f"AI-customized detection settings:")
    print(f"  Sensitivity: {ai_settings.sensitivity}")
    print(f"  Min size: {ai_settings.min_size} pixels")
    print(f"  Fuse distance: {ai_settings.fuse_distance_m}m")
    print()
    
    # AI can calculate threshold for any data
    test_mean = 280.0  # Kelvin
    test_std = 5.0
    threshold = ai_settings.get_threshold(test_mean, test_std)
    print(f"For data with mean={test_mean}K, std={test_std}:")
    print(f"  AI would detect pixels > {threshold:.2f}K")
    print()
except ImportError as e:
    print(f"Note: huron_detect_and_fuse requires rasterio, scipy")
    print(f"  Install with: pip install rasterio scipy")
    print()

# =============================================================================
# SCENARIO 5: AI saves/loads configuration
# =============================================================================

print("SCENARIO 5: AI saves configuration for reuse")
print("-"*70)

config_path = Path('outputs/ai_config.json')
config_path.parent.mkdir(parents=True, exist_ok=True)

settings.save(config_path)
print(f"AI saved config to: {config_path}")

# Load it back
settings_loaded = GlobalScannerSettings()
settings_loaded.load(config_path)
print(f"AI loaded config: {settings_loaded.to_dict()}")
print()

print("="*70)
print("DEMONSTRATION COMPLETE")
print("="*70)
print()
print("AI AGENT CAN NOW:")
print("  1. Adjust sensitivity for different lakes/targets")
print("  2. Call curvelet transform with custom parameters")
print("  3. Call detection/fusion with custom thresholds")
print("  4. Save/load configurations")
print("  5. Chain tools together in pipelines")
print()
