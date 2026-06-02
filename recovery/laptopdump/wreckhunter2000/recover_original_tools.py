"""
recover_original_tools.py

Recovers original working CLI tools from clean backups.
Copies from dist/WreckHunter2000_Clean/ and extracted zip files.

DOES NOT DELETE ANYTHING - only adds to recovered_originals/
"""

import shutil
from pathlib import Path

# ── Paths ─────────────────────────────────────────────────────────────────────

REPO = Path(__file__).resolve().parent
CLEAN_SOURCE = Path('c:/Users/thomf/programming/Bagrecovery/dist/WreckHunter2000_Clean')
RECOVERED_DIR = REPO / 'recovered_originals'
RECOVERED_DIR.mkdir(parents=True, exist_ok=True)

# ── Tools to Recover ──────────────────────────────────────────────────────────

TOOLS_TO_RECOVER = {
    'mag_tools': {
        'description': 'Magnetic anomaly detection CLI tools',
        'files': [
            CLEAN_SOURCE / 'mag_pipeline_validator.py',
            CLEAN_SOURCE.parent / 'mag_pipeline_validator.py',  # Also in root
        ],
        'destination': RECOVERED_DIR / 'mag_tools',
    },
    'pdf_redaction_breaker': {
        'description': 'PDF redaction breaker (NOAA BAG files)',
        'files': [
            CLEAN_SOURCE / 'bag_processor' / 'pdf_redaction_breaker.py',
            CLEAN_SOURCE / 'bag_processor' / 'pdf_bag_crossref.py',
        ],
        'destination': RECOVERED_DIR / 'pdf_redaction_breaker',
    },
    'bagfile_scanner': {
        'description': 'BAG file scanner with 3D KML output',
        'files': [
            CLEAN_SOURCE / 'bag_processor' / 'bag_wreck_detector.py',
            CLEAN_SOURCE / 'bag_processor' / 'bag_visualization_generator.py',
            CLEAN_SOURCE / 'bag_processor' / 'bag_metadata_analyzer.py',
        ],
        'destination': RECOVERED_DIR / 'bagfile_scanner',
    },
    'bag_processor_rust': {
        'description': 'Rust-accelerated BAG processor (4-minute miracle)',
        'files': [
            RECOVERED_DIR / 'bag_processor_rust' / 'src' / 'lib.rs',
            RECOVERED_DIR / 'bag_processor_rust' / 'Cargo.toml',
        ],
        'destination': RECOVERED_DIR / 'bag_processor_rust',
    },
}

# ── Recovery Functions ────────────────────────────────────────────────────────

def recover_tool(tool_name: str, tool_config: dict) -> dict:
    """Recover a single tool."""
    result = {
        'tool': tool_name,
        'description': tool_config['description'],
        'recovered': [],
        'missing': [],
        'errors': [],
    }
    
    dest_dir = tool_config['destination']
    dest_dir.mkdir(parents=True, exist_ok=True)
    
    for src_file in tool_config['files']:
        if src_file.exists():
            try:
                # Copy file
                dest_file = dest_dir / src_file.name
                shutil.copy2(src_file, dest_file)
                result['recovered'].append(str(dest_file))
                print(f'  ✓ {src_file.name}')
            except Exception as e:
                result['errors'].append(f'{src_file.name}: {e}')
                print(f'  ✗ {src_file.name}: {e}')
        else:
            result['missing'].append(str(src_file))
            print(f'  ⚠ {src_file.name} (not found)')
    
    return result


def main():
    """Recover all tools."""
    print('='*70)
    print('RECOVERING ORIGINAL WORKING TOOLS')
    print('='*70)
    print()
    
    all_results = []
    
    for tool_name, tool_config in TOOLS_TO_RECOVER.items():
        print(f'Recovering {tool_name}...')
        print(f'  {tool_config["description"]}')
        result = recover_tool(tool_name, tool_config)
        all_results.append(result)
        print()
    
    # Summary
    print('='*70)
    print('RECOVERY SUMMARY')
    print('='*70)
    
    for result in all_results:
        print(f"\n{result['tool']}:")
        print(f'  Recovered: {len(result["recovered"])} files')
        if result['missing']:
            print(f'  Missing: {len(result["missing"])} files')
        if result['errors']:
            print(f'  Errors: {len(result["errors"])} files')
    
    print()
    print(f'All recovered files in: {RECOVERED_DIR}')
    print('='*70)


if __name__ == '__main__':
    main()
