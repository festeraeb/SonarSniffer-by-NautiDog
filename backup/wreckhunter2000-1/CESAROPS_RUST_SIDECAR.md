# CESAROPS: Rust Architecture & Execution Sidecar
*Last Updated: April 20, 2026*

## 🎯 The Pivot: Why Rust + WGPU?
We encountered blockers with remote bare-metal Ubuntu environments (UEFI Secure Boot breaking native NVIDIA/CUDA driver installations). To eliminate dependencies on `nvcc` and fragile CUDA environments, we pivoted the local processing architecture completely to **Rust + WGPU/Vulkan**. WGPU allows us to deploy the same compute shaders across any hardware (GTX, M2200) without compiling C++ CUDA code.

## 🏗️ The Multi-Device Batch Architecture
Hardware constraint: We have exactly an 8GB GTX card, plus secondary devices like a Coral TPU and Quadro M2200. We cannot run heavy optical Vulkan scanning concurrently with KoboldCPP/Llama-3 in the same 8GB VRAM footprint. 

**Solution: Decoupled Producer-Consumer Pipeline**
1. **Phase 1: Distributed Scoring (Producers)**
   - The Rust WGPU/Rayon workers (GTX), Coral TPU, and M2200 run purely as asynchronous *Producers*. 
   - They scan STAC data, perform the "Triple-Lock" physics checks, and dump suspected anomalies into a pending database/queue (`sled` or SQLite/Postgres). No LLM loads happen here.
2. **Phase 2: Assessment (Consumer)**
   - All optical scanners shut down, freeing 100% of VRAM.
   - Orchestrator spawns `koboldcpp.exe`.
   - The Rust `llm_worker.rs` kicks in as a consumer, pulling pending anomalies from the queue and running RAG/specialist prompts to evaluate every hit. 
3. **Phase 3: Teardown**
   - Kill KoboldCPP. Resume scanning.

---

## 🚥 Worker Status (cesarops-slicer)

### ✅ Done / Stubbed
- **`Cargo.toml`**: Fully scaffolded with all dependencies (`wgpu`, `rayon`, `tokio`, `sled`, `stac`, `oxigdal`, etc.) and binaries mapped.
- **`llm_worker.rs`**: Created the consumer loop structure utilizing an embedded `sled` database to pull specialized target instructions and send POST requests to the Kobold API.
- **`sar_slick_worker.rs`**: Rayon multithreaded algorithm assessing capillary wave dampening for localized variance (Sentinel-1).
- **`bathymetry_specialist.rs`**: Rayon-accelerated gradient analysis for BAG files (HDF5) identifying sharp artificial edges / wrecks over sand slopes.
- **`reef_anomaly_filter.rs`**: False-positive filter computing Haversine distances to cross-reference STAC hits against known artificial reef coordinates to avoid intentional sinkings.
- **`thermal_specialist.rs`**: WGPU scanner evaluating Landsat 8/9 cold-sink anomalies. WGSL shader stubbed out. WGPU device/queue adapter acquisition code is complete.
- **`optical_structural_worker.rs`**: CPU stub complete, WGPU Sobel edge-variance WGSL shader included but needs buffer mappings.

### ⏳ Pending / Work to Complete (Legacy)
1. **Flesh out WGPU Buffer Mappings in Scanners**: `optical_structural_worker` has the WGSL shader but is using a CPU stub to return empty results. We need to implement the buffer pushing/pulling for WGPU.
2. **Central Anomaly Queue**: Standardize the message format (JSON interface + db row) so all workers (SAR, Optical, Thermal) pipe into the same `status='llm_analysis_pending'` queue.
3. **n8n / Process Orchestration**: Scaffold the scripts or n8n nodes that formally execute the pipeline stages (launch workers -> wait for completion -> load LLM -> launch LLM worker).
4. **Chemistry Specialist**: Add `chemistry_specialist.rs` if needed for PRISMA/DESIS (Iron-Oxide/Lead salt checks).
5. **Testing**: Run local tests for each worker binary individually against a known target (e.g., *Lumberman*).

## 🚫 Critical Constraints
- **NO CUDA (`nvcc`)**: Do not drift back to PyCUDA or native C++ CUDA requirements. Rely strictly on `rayon` (CPU) or `wgpu` (Vulkan/DX12).
- **NO SYNCHRONOUS LLMs**: Do not hold up a scan waiting for the LLM. Produce anomaly -> queue -> evaluate later.

---

## 🏆 Current State vs. Best Goal Analysis

### The "Best Goal" Vision
A purely decoupled, hyper-localized scanning engine written 100% in Rust. It utilizes low-level GPU compute (`WGPU`/Vulkan) for zero-copy massive parallel pixel analysis (Optical, Thermal, Bathymetry) and CPU multithreading (`Rayon`) for less parallel workloads (SAR). 
It strictly follows the "Triple-Lock" physics methodology from the research paper without ever blocking on LLMs. Anomalies are dumped into a high-speed embedded database (`Sled`) acting as an asynchronous queue. 
Once scanning runs out of memory/time, or completes a sector, **all scanners terminate -> 8GB VRAM is 100% freed -> LLM boots via KoboldCPP -> Consumer `llm_worker` evaluates every queued hit via specialized prompt context -> LLM unloads -> Scanning resumes.** No CUDA headaches, no bare-metal SSH wrestling, 100% scalable asynchronous logic on limited hardware.

### Where we are at
- **Language/Environment:** We successfully migrated from Python/CUDA dependencies to a standard Rust `Cargo` workspace.
- **Workers:** We have drafted the foundational algorithms for Optical, Thermal, Bathymetry, SAR, Reef Filters, and the LLM Consumer. The mathematical detection intent is mostly written.
- **The Bottleneck:** The primary flaw right now is that the workers process **mock data** (e.g., `vec![-12.0; 1000]`). They do not yet accept real Earth-observation GeoTIFF chunks. Furthermore, the WGPU shaders are initialized but the raw `byte-slice` I/O buffering from RAM to VRAM is not fully wired up. Last, the anomaly database (`Sled`) is isolated per-file instead of being a unified interface.

---

## 🗿 The "Locked In Stone" Steps Forward

To bridge the gap between our current state and the ultimate "Best Goal", we must execute strictly in this order:

1. **Unify the Anomaly Queue (The `Sled` Core)**
   - Create a shared `common/db.rs` or `queue.rs` library.
   - Define a universal `struct AnomalyRecord { lat, lon, bbox, sensor_type, confidence_score, imagery_path, status }`.
   - Wire all existing Producers (Optical, Thermal, SAR, etc.) to serialize their findings and push them into this single local Sled database with a status of `pending_llm`.

2. **Real STAC / GeoTIFF I/O Ingestion**
   - Eliminate the `vec![]` mock arrays in the workers.
   - Use the `oxigdal` (GDAL) or `stac` crates to write a helper function that downloads/streams actual imagery slices from Microsoft Planetary Computer (Sentinel/Landsat) and converts them into flattened `Vec<f32>` arrays.

3. **Plumb the WGPU Memory Buffers**
   - Inside `optical_structural_worker` and `thermal_specialist`, take the real `Vec<f32>` array from Step 2, map it into a `wgpu::Buffer`, dispatch the WGSL compute pass, and map the memory back as async read. This proves the GPU computation works end-to-end natively.

4. **Local Verification on a Known Target**
   - Run the full Producer pipeline against a known wreck coordinate (e.g., *The Lumberman*). Ensure the Optical, SAR, and Thermal workers all fire, process real pixels, and dump hits into the `Sled` database.

5. **The LLM Consumer Handoff**
   - With real records waiting in the database, boot KoboldCPP locally.
   - Run `llm_worker.rs` to pull the records, construct the specialized Triple-Lock prompts, hit the Kobold API, log the final assessment, and terminate. This proves the decoupled batch architecture works flawlessly.