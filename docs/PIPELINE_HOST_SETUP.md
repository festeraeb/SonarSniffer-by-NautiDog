# Pipeline host setup (cesarops2)

What this box needs to **build and run** the three detection pipelines plus Forge.

## One-shot install

```bash
cd /data/codebase/repos/wreckhunter2000-1
bash scripts/install_pipeline_host_deps.sh --build
```

Check only (no apt):

```bash
bash scripts/install_pipeline_host_deps.sh --check
```

Heavy Python stack (rasterio, etc.) for legacy scripts:

```bash
INSTALL_PY_PIPELINE=1 bash scripts/install_pipeline_host_deps.sh
```

## Per-crate requirements

| Program | System | Rust | Runtime |
|---------|--------|------|---------|
| **cesarops-satellite** (`sat-run`) | None required (pure-Rust TIFF chips). Optional: `libgdal-dev` if built with `--features gdal` | Workspace member | `scripts/credentials.sh` for download stage; `python3` + `universal_downloader.py` + `requests` |
| **cesarops-aeromagnetic-worker** | Vulkan (`libvulkan1`, drivers) for WGPU dipole path | Workspace member | Aeromag grid `.bin` + optional OGSr wells CSV |
| **cesarops-bag-scan** | **`libgdal-dev`** + `gdal-bin` (provides `gdal.pc` for `gdal-sys`) | Standalone crate under `cesarops-bag-scan/` | `.bag` file path |
| **cesarops-forge-v2** | — | Workspace member | `systemctl` service on `:9100` |

### GDAL on Ubuntu

`gdal-bin` alone is **not enough** to compile `cesarops-bag-scan`. You need **`libgdal-dev`** so `pkg-config --modversion gdal` succeeds.

```bash
sudo apt-get install -y libgdal-dev gdal-bin gdal-data pkg-config
```

## Binaries after build

| Binary | Typical path |
|--------|----------------|
| `sat-run` | `target/release/sat-run` |
| `cesarops-aeromagnetic-worker` | `target/release/cesarops-aeromagnetic-worker` |
| `cesarops-bag-scan` | `cesarops-bag-scan/target/release/cesarops-bag-scan` (standalone build) |
| `cesarops-forge-v2` | `target/release/cesarops-forge-v2` |

## Credentials

```bash
source /data/codebase/repos/wreckhunter2000-1/scripts/credentials.sh
```

- Satellite download: `EARTHDATA_*`, `COPERNICUS_*`, `USGS_*` (see `universal_downloader.py`)
- Spec Thinker: `scripts/credentials.gemini.local.sh` (gitignored)

## Smoke tests

```bash
source scripts/credentials.sh
sat-run --describe | head -20
cesarops-aeromagnetic-worker describe | head -20
cesarops-bag-scan --describe | head -20
cargo test -p cesarops-satellite
cargo test -p cesarops-aeromagnetic-worker
(cd cesarops-bag-scan && cargo test)
```

## Fleet / Forge

- Forge: `http://127.0.0.1:9100` — `systemctl status cesarops-forge-v2`
- Spec draft: `POST /spec/draft`, `POST /spec/approve`
- Unified fleet: `bash scripts/fleet status`
