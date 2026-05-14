Utility scripts for SonarSniffer

scan_garmin_firmware.py
- Use this to scan firmware blobs (e.g., your `garminjunk` folder) for frequent 4-byte sequences which may indicate alternate header magics.
- Example:
  python scripts/scan_garmin_firmware.py garminjunk --min-count 30 --top 10 --out garmin_magic_variants.txt
- After reviewing `garmin_magic_variants.txt` you can add it to the repo root as `garmin_magic_variants.txt` so the parser will auto-load those alternatives at runtime.
