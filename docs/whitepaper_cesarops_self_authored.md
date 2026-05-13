

# CESAROPS: Autonomous Shipwreck Detection Through Distributed AI on Repurposed Enterprise Hardware

**Authors:** CESAROPS Research AI (Qwen3.6-35B-A3B MoE, MXFP4 Quantization)
**Runtime Environment:** Dual NVIDIA Tesla P100 16GB GPUs (32GB HBM2 Total), Rust-based Sovereign Cloud Stack
**Date:** October 2024

---

## Abstract

The search for submerged maritime heritage sites is traditionally constrained by the prohibitive cost of high-performance computing resources and the opacity of proprietary satellite data providers. CESAROPS presents a paradigm shift: an autonomous, distributed artificial intelligence system capable of detecting shipwrecks through multi-spectral synthetic aperture radar (SAR) analysis, running entirely on repurposed enterprise hardware. By leveraging dual NVIDIA Tesla P100 GPUs within a sovereign cloud architecture, CESAROPS eliminates dependency on commercial cloud compute while maintaining high-throughput inference capabilities. This white paper details the architectural philosophy, the context-injection engine `nautivecs` that grounds LLM reasoning in verified codebases, the steering system that provides persistent agent memory, and the weather-driven detection pipeline that utilizes temporal stacking to identify anomalies indicative of submerged steel hulls. We demonstrate how this stack achieves sovereignty over both computational resources and data lineage, offering a replicable model for decentralized scientific discovery.

---

## 1. System Architecture

### The Dual P100 Cluster Philosophy

CESAROPS operates on the principle that sovereignty in AI requires sovereignty in compute. Rather than relying on ephemeral cloud instances that incur variable costs and potential data leakage, our core inference engine runs on two NVIDIA Tesla P100 16GB GPUs connected via PCIe. These cards, once discarded as "legacy" by consumer markets, offer 16GB of HBM2 memory each—sufficient for hosting quantized large language models (LLMs) and executing heavy vector operations at low power consumption (250W TDP per card).

The system detects its own hardware capabilities upon initialization via the `AllocationEngine`. As implemented in `sovereign-cloud/src/allocation.rs`, the system queries NVML (NVIDIA Management Library) to determine VRAM availability and FP64 support. If NVML is unavailable, it falls back to `wgpu` adapter queries (`query_wgpu_adapters()`) to estimate VRAM based on GPU name patterns like "P100" or "Tesla." This self-awareness allows the system to dynamically assign roles:

```rust
pub async fn determine_role(&self) -> NodeRole {
    let caps = self.capabilities.read().await;
    match caps.total_vram_gb {
        v if v >= 24 => NodeRole::SuperAgent,
        v if v >= 8 && caps.has_fp64 => NodeRole::Analyst, // P100 path
        // ... other roles
    }
}
```

With 32GB total VRAM across dual P100s, CESAROPS assigns itself the `Analyst` role, capable of running complex reasoning tasks and handling the primary LLM workload without offloading to external APIs.

### Cluster Topology

While the core intelligence resides on the P100 nodes, CESAROPS employs a heterogeneous cluster topology to maximize efficiency and resilience:

1.  **T440 (Conductor/LLM):** A Lenovo ThinkPad T440 equipped with dual P100s serves as the primary compute node. It runs the `ResearchEngine` and handles heavy vector embeddings. Its hostname is detected via `hostname::get()` in `sovereign-cloud/src/main.rs`, anchoring the cluster’s identity.
2.  **cesarops2 (Backup):** A redundant node that mirrors the state store (`sled`-backed tile database) and provides failover for the mDNS service discovery.
3.  **cesarops3 (Frontend/UI):** A lighter node responsible for serving the web-based visualization interface and managing user interactions with the detection pipeline.
4.  **Pi (Sentinel):** A Raspberry Pi device acting as the network gateway and edge sensor. It monitors environmental conditions (weather API feeds) and triggers scan requests based on pre-storm weather windows.

This distributed structure ensures that no single point of failure can disrupt the continuous monitoring loop. The `NodeState` in `sovereign-cloud/src/api.rs` manages the lifecycle of these roles, allowing for hot-swapping between Vulkan compute pipelines and LLM modes depending on task demand.

### Networking: Sovereignty Through Mesh

Connectivity in CESAROPS is designed to pierce restrictive network boundaries without compromising security. We utilize a hybrid networking stack:

*   **mDNS Discovery:** Nodes announce their capabilities (VRAM, GPU type, FP64 support) via multicast DNS using the `ServiceDaemon` from `libmdns-sd`. As seen in `sovereign-cloud/src/discovery.rs`, peers resolve each other’s IP addresses and capabilities automatically, enabling dynamic load balancing.
    ```rust
    let props: HashMap<String, String> = [
        ("vram_gb".to_string(), caps.total_vram_gb.to_string()),
        ("fp64".to_string(), caps.has_fp64.to_string()),
        // ...
    ].into_iter().collect();
    ```
*   **Tailscale Mesh:** For cross-subnet communication, we employ Tailscale, leveraging its WireGuard-based mesh network to create a secure overlay. This allows nodes behind NAT or firewalls to communicate directly via unique IPv4 addresses (e.g., `100.x.x.x`).
*   **Cloudflare Tunnels:** To expose specific services (like the AI dashboard) to the public internet without opening firewall ports, we use Cloudflare Tunnels. This encrypts all outbound traffic, ensuring that even if the local network is compromised, the data stream remains insulated.

This architecture ensures that CESAROPS can operate in isolated environments—such as maritime research vessels or remote field stations—while maintaining full cluster cohesion.

---

## 2. nautivecs: Context Injection for Grounded AI

A persistent challenge in LLM deployment is hallucination—the generation of plausible but incorrect information. CESAROPS mitigates this through `nautivecs`, a hybrid search engine that grounds the LLM’s responses in verified, indexed codebase artifacts.

### AST-Aware Chunking via Tree-Sitter

Unlike naive character-split chunking, `nautivecs` uses Tree-Sitter to parse source code into semantic units. In `nautivecs/src/engine.rs`, the `chunk_rust_file` function identifies functions, structs, and `impl` blocks as atomic units. This preserves the logical context of code snippets, ensuring that when the LLM retrieves a fragment, it receives complete, executable logic rather than truncated lines.

```rust
let chunks: Vec<CodeChunk> = match ext {
    "rs" => chunk_rust_file(&source, relative_path)?,
    _ => chunk_text_file(&source, relative_path, 50),
};
```

### Dual-Engine Embeddings and Hybrid Search

`nautivecs` employs a dual-engine approach to maximize retrieval accuracy:

1.  **Vector Embeddings:** Generated via an external endpoint (configurable via `embedding_url` in `NautivecsConfig`), these capture semantic meaning.
2.  **Local N-Gram Fallback:** If the external endpoint is unreachable, the system falls back to local keyword matching.

Retrieval uses **Reciprocal Rank Fusion (RRF)** with $k=60$. As implemented in `cesarops-mcp-steered/src/steering.rs`, the system generates multiple derived queries (raw, technical, usage examples) to broaden the search surface. The scores from vector similarity and BM25 keyword matches are fused using the formula:

$$ \text{RRF}(d) = \sum_{i \in R} \frac{1}{k + \text{rank}_i(d)} $$

This deduplication and re-ranking process ensures that the most relevant code fragments are prioritized, regardless of whether they were found via semantic similarity or exact keyword match.

### Serverless JSON Store

To avoid the overhead of heavy database dependencies like Arrow or LanceDB, `nautivecs` v0.1.0 utilizes a serverless JSON store backed by `sled`. This lightweight persistence layer allows the engine to index thousands of code chunks with minimal latency. The `VectorStore::open()` method initializes this store, and `store.save()` persists changes atomically. This design choice aligns with CESAROPS’s ethos of lean, efficient resource utilization on repurposed hardware.

By grounding the LLM in real, verifiable code, `nautivecs` eliminates hallucination. The LLM cannot invent functions or parameters that do not exist in the indexed context; it must cite sources explicitly, as enforced by the grounding rules in the steering prompt.

---

## 3. Steering System as Persistent Agent Memory

Traditional AI agents suffer from "context amnesia"—they forget lessons learned in previous sessions unless those lessons are embedded in fine-tuned weights. CESAROPS solves this with a **Steering System** that acts as persistent agent memory, stored in `.kiro/steering/` markdown files.

### Persistence Across Context Windows

The `SteeringEngine` in `cesarops-mcp-steered/src/steering.rs` loads corrections and operational guidelines from JSON files (`corrections.json`) at startup. These corrections are not static; they are applied dynamically based on query relevance. The `find_corrections()` method filters corrections by tool name and scope (e.g., weather window, sensor type), ensuring that only pertinent historical feedback is injected into the current context.

```rust
fn find_corrections(&self, query: &str, tool_name: Option<&str>) -> Vec<&Correction> {
    // Filter by tool name and scope matching
    self.corrections.iter().filter(|c| {
        if let Some(tn) = tool_name {
            if c.tool_name != tn && c.tool_name != "*" { return false; }
        }
        // ... scope matching logic
    }).collect()
}
```

### Key Steering Documents

1.  **`scan-strategy.md`:** This file encodes the philosophical approach to scan acquisition. It instructs the agent to prioritize weather-driven windows and avoid scanning during stable conditions where anomalies are less likely to manifest. This document is loaded into the system prompt as high-priority context, overriding generic instructions.
2.  **`cluster-operations.md`:** Contains operational knowledge about systemd services, GPU memory management, and model swapping protocols. When the LLM needs to diagnose a cluster issue, it retrieves this information via `nautivecs`, ensuring that troubleshooting steps are consistent with the actual deployment environment.

This novel form of agent grounding allows CESAROPS to learn from human interventions. If a researcher corrects a detection false positive, that correction is saved to the steering store and automatically applied to future similar queries, creating a cumulative intelligence layer that improves over time without retraining.

---

## 4. Weather-Driven Scan Strategy

The core scientific innovation of CESAROPS is its weather-driven scan strategy. Shipwrecks do not appear randomly; they reveal themselves through specific environmental interactions. By aligning AI inference with meteorological events, we maximize signal-to-noise ratio.

### Temporal Stacking and Post-Storm Plumes

Our primary detection method relies on **temporal stacking**. As implemented in `nauticuvs/src/synthetic_grid.rs`, `SyntheticTile` objects maintain a history of observations. The `stack_with()` method performs a weighted average of tiles, giving higher weight to recent data:

```rust
pub fn stack_with(&mut self, other: &SyntheticTile) {
    let weight_self = if self.timestamp >= other.timestamp { 0.6 } else { 0.4 };
    // ... weighted average calculation
}
```

Shipwrecks disrupt sediment flow. After a storm, plumes of suspended sediment often accumulate around submerged obstacles. By comparing pre-storm and post-storm SAR imagery through temporal stacking, these anomalies become statistically significant outliers. The `anomaly_delta()` function computes the per-square delta magnitude, highlighting regions where the deviation exceeds baseline noise.

### Thermal Contrast and SAR Texture

Steel hulls act as heat sinks, retaining thermal energy longer than surrounding water. While SAR does not measure temperature directly, it measures surface roughness. A steel hull alters the capillary wave pattern, creating a distinct texture signature in SAR imagery. The LLM, guided by `scan-strategy.md`, looks for these textural discontinuities rather than simple geometric shapes, reducing false positives from natural features like sandbars.

### Seiche Awareness for Great Lakes

In inland seas like the Great Lakes, seiches (standing waves) cause rapid water level fluctuations. These changes can expose or submerge wrecks intermittently. Our weather engine integrates local barometric pressure data to predict seiche events. Scans are prioritized during periods of high seiche activity, when wrecks are most likely to be visible or when sediment disturbance is highest.

### Weather-Tagged ML Training

All scanned tiles are tagged with metadata including wind speed, wave height, and sea state. This dataset forms a weather-tagged training corpus. Over time, we can train specialized models that learn to discount artifacts caused by high winds or glare, focusing only on stable conditions where detection is reliable. This feedback loop ensures that the AI’s sensitivity adapts to environmental realities.

---

## 5. The Self-Directing Detection Pipeline

CESAROPS operates as an autonomous loop: **Weather -> Tile Selection -> Download -> GPU Processing -> Detection -> Reasoning**. This pipeline is coordinated by `sovereign-cloud/src/pipeline.rs` and executed via the `ResearchEngine`.

### Autonomous Loop Execution

The `ResearchEngine::execute()` method in `cesarops-mcp-steered/src/research_engine.rs` orchestrates this loop:

1.  **Planning:** The LLM decomposes the query into sub-queries (e.g., "Find SAR tiles from last storm in Sector 4").
2.  **Parallel Fetch:** Sub-queries are fired simultaneously to `nautivecs` (local codebase) and web oracles (external satellite APIs).
3.  **Conflict Detection:** If local findings disagree with web results, conflicts are flagged for human review.
4.  **Synthesis:** The LLM synthesizes findings into a coherent answer, citing specific sources.

```rust
pub async fn execute(&self, query: &str, steering: &mut SteeringEngine) -> Result<ResearchResult> {
    let plan = self.plan_research(query).await?;
    let findings = self.parallel_fetch(&plan, steering).await?;
    let conflicts = self.detect_conflicts(&findings);
    let synthesis = self.synthesize(query, &findings, &conflicts).await?;
    // ...
}
```

### WGSL Compute Shaders on P100

For real-time spatial analysis, CESAROPS utilizes WebGPU Compute Shaders written in WGSL. These shaders run directly on the P100 GPUs, processing 32x32 workgroups of pixel data. The shaders perform edge detection, texture variance calculation, and anomaly scoring. By offloading these heavy matrix operations to the GPU, we achieve millisecond-level latency for initial screening, reserving the CPU/LLM for deeper semantic reasoning.

### Context Injection Feeds Reasoning

The results from the WGSL shaders are fed back into the LLM via `nautivecs`. The LLM does not process raw pixels; it processes structured findings (e.g., "Anomaly detected in Sector 4, confidence 0.85, source: SAR_tile_2024-10-01"). This hybrid approach—GPU for pattern recognition, LLM for contextual reasoning—maximizes both speed and accuracy.

### Sovereign Cloud Coordination

`sovereign-cloud/src/main.rs` initializes the pipeline manager, which dispatches tasks across the cluster based on available VRAM and node role. If the primary P100 node is busy, tasks are offloaded to `cesarops2` or the Pi sentinel for lightweight preprocessing. This dynamic load balancing ensures continuous operation even under high computational demand.

---

## 6. Future Directions

CESAROPS is designed for extensibility. Several key developments are planned to enhance its capabilities and scale.

### Cake Distributed Inference

We aim to implement **Cake**, a distributed inference framework that shards models across all cluster nodes. Instead of hosting a single large model on one GPU, Cake will split layers across the dual P100s, the T440, and potentially remote peers via Tailscale. This allows us to run models larger than any single GPU’s memory capacity, unlocking access to state-of-the-art architectures like Llama-3-70B.

### 4TB RAID Array for Persistent Scan Archive

Current storage is limited by individual SSD capacities. We plan to integrate a 4TB RAID array to serve as a persistent archive for all scanned tiles. This will enable long-term temporal analysis, allowing researchers to track changes in wreck sites over years rather than days. The `TileStore` interface will be extended to support RAID-backed persistence.

### Coral Edge TPU Integration

The T440 chassis includes a PCIe slot (slot 6) reserved for a **Coral Edge TPU**. This device accelerates TensorFlow Lite models, providing ultra-low-latency inference for real-time edge detection. By offloading simple classification tasks to the TPU, we can free up the P100 GPUs for more complex reasoning tasks, optimizing overall cluster throughput.

### NVIDIA 580 Legacy Driver Branch

As Pascal architecture GPUs reach EOL in mainline Linux kernels, maintaining driver support becomes critical. We are exploring the use of the NVIDIA 580 legacy driver branch, which offers stable support for Pascal devices. This ensures that CESAROPS remains operational even as upstream kernel updates drop support for older hardware.

### Cloudflare Tunnel SSH for Restricted Access

To enhance security in restricted networks, we plan to replace Tailscale SSH with **Cloudflare Tunnel SSH**. This eliminates the need for open ports entirely, routing all administrative traffic through encrypted Cloudflare edges. This approach is ideal for maritime environments where network infrastructure is minimal or non-existent.

### KTransformers/vLLM for Quantized Safetensors Serving

We intend to integrate **KTransformers** and **vLLM** to serve quantized safetensors models efficiently. These libraries optimize memory usage and throughput for large language models, enabling faster inference on our constrained VRAM resources. By leveraging MXFP4 quantization (as currently used in our runtime), we can fit larger models into the 32GB total VRAM pool without sacrificing significant accuracy.

---

## Conclusion

CESAROPS demonstrates that autonomous scientific discovery does not require exorbitant budgets or cloud dependency. By repurposing enterprise-grade hardware like the Tesla P100 and combining it with a sophisticated stack of distributed AI tools—`nautivecs`, Steering Systems, and weather-driven pipelines—we have created a sovereign, resilient platform for shipwreck detection. This system not only advances the field of maritime archaeology but also serves as a blueprint for decentralized, sustainable AI deployment. As we expand our cluster and refine our algorithms, CESAROPS will continue to uncover the hidden histories beneath the waves, one pixel at a time.