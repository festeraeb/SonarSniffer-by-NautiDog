# CESAROPS Pipeline Unification — Architecture

End goal: a user logs in, tells an LLM what they're looking for (a specific wreck,
a debris field, a downed aircraft, an anomaly in a lake), and the LLM orchestrates
the right pipelines — each a Rust tool with tunable, selectable options — then an
n8n agent automates the run and reports findings.

This doc defines the shared contract every pipeline implements so the LLM and n8n
can drive them uniformly. It is grounded in the pattern already proven in
`cesarops-satellite` (mission spec + knobs + stages + report).

## 1. The five pipelines (targets)

| Pipeline | Crate | Status | Purpose |
|----------|-------|--------|---------|
| Satellite | `cesarops-satellite` | mature (~4.8k ln, mission pattern done) | optical/SAR wreck + SAR-rescue detection |
| Aeromagnetic (mag) | `cesarops-aeromagnetic-worker` | partial (~1.8k ln) | magnetic anomaly detection (ferrous wrecks) |
| BAG / bathymetry sonar | `cesarops-bag-scan` | stub (~250 ln) | multibeam BAG scan, seafloor target detection |
| PDF breaker | (new) `cesarops-pdf-extract` | not started | extract survey data/coords from PDFs & reports |
| SonarSniffer | `sonarsniffer` | mostly built (~32k ln) | side-scan/RSD parsing, mosaic, curvelet detect |

Supporting: `cesarops-detection` + `jitter-rs` (cross-validated inference),
`nauticuvs` (shared geo/DSP math), `cesarops-agent` (orchestration glue).

## 2. The shared pipeline contract

Every pipeline crate exposes the SAME four concepts (the satellite crate is the
reference implementation):

1. **`MissionSpec`** — JSON input: `mission_id`, target, `bbox`, date range,
   `stages`, `knobs` (overrides), `paths`. Serde-(de)serializable.
2. **`Knobs`** — all tunable parameters with sane defaults (`#[serde(default)]`).
   This is the LLM's "specialist dial set" — every knob is documented and
   range-checked.
3. **`Stage` enum** — selectable, ordered pipeline stages. The LLM/n8n can run a
   subset (e.g. just `download` + `report`, or skip straight to `validate`).
4. **`MissionReport`** — structured JSON findings: status, runtime, per-stage
   results, ranked `Candidate`s. This is what n8n forwards back to the user.

Each crate ships a CLI binary (`<pipe>-run`) with the same flags:
`--spec`, `--knobs '<json>'`, `--stages ...`, `--dry-run`, `--root`,
`--preflight`. Stdout = human summary; `--json` = machine report for n8n.

## 3. LLM / n8n orchestration layer

```
  user ──▶ LLM (intent → plan)
              │  picks pipelines + knobs from a tool catalog
              ▼
        n8n agent (per-pipeline specialist nodes)
              │  invokes <pipe>-run --spec … --json
              ▼
     Rust pipeline crates  ──▶  MissionReport JSON
              │
              ▼
        LLM (synthesize findings) ──▶ user report
```

- **Tool catalog**: each pipeline publishes a machine-readable descriptor
  (`<pipe>-run --describe` → JSON) listing its stages, every knob with type +
  range + default + one-line help, and required auth/inputs. The LLM reads this
  to know what it can tune. This makes each pipeline self-documenting to the
  orchestrator — no hardcoded prompt knowledge.
- **Specialist mapping**: one n8n sub-workflow per pipeline, plus a router that
  the LLM drives. n8n already wired in the repo (`fleet-n8n-dispatch.sh`).
- **MCP bridge**: `cesarops-mcp-worker` / `cesarops-mcp-steered` can surface the
  same `--describe`/`run` surface as MCP tools so any MCP-capable LLM calls them
  directly.

## 4. Standardization plan (incremental, low-risk)

Phase A — **contract crate**: extract the satellite `Knobs/Stage/MissionSpec/
MissionReport` shape into a tiny shared crate `cesarops-pipeline-core` (traits +
the `--describe`/`--json` plumbing). No behavior change to satellite.

Phase B — **retrofit mature crates**: make `cesarops-aeromagnetic-worker` and
`sonarsniffer` implement the contract (they already have most logic; this is
wrapping it in MissionSpec/Report + `--describe`).

Phase C — **build the stubs to spec**: `cesarops-bag-scan` (port the Python
bag_processor logic we just migrated under `pipelines/bag/`), and a new
`cesarops-pdf-extract` (PDF → coords/survey-data, feeding the other pipelines).

Phase D — **orchestration**: `--describe` tool catalog → n8n specialist nodes →
LLM router. End-to-end "ask → run → report".

## 5. Why Rust, and the accelerator story

- Pure-Rust primary paths (tract/ndarray/rustfft) — portable, no fragile C deps.
- Heterogeneous accelerators are *validators*, not hard deps (see `jitter-rs` +
  `CORAL_TPU_WORKER_SPEC.md`): CPU/GPU primary, Coral TPU + Movidius corroborate.
- GPU via `wgpu` (Vulkan) is the forward path; Pascal/Myriad are EOL and stay
  optional behind feature flags.

## 6. Source-of-truth notes

- Migrated recovery files: P0 → `pipelines/{mag,bag,satellite}` (live, via
  `/codebase/projects/pipelines`); P2 → `recovery/laptopdump/` (staging). See
  `var/fleet-catalog/rs_pipeline_recovery/MIGRATION_DONE.md`.
- Python under `pipelines/` is the behavioral reference to port FROM; the Rust
  crates are the destination. Keep Python until the Rust stage reaches parity,
  then retire per-stage.
- Older git repos under the `festeraeb` account may hold earlier versions if a
  file looks truncated here — check there before reconstructing from scratch.
```
