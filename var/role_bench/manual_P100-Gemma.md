## Goal
Automate the classification of ~185 shell scripts currently residing on shared NFS. The objective is to generate a machine-readable `forge-verdict.json` that categorizes every script into one of three tiers (`keep`, `archive`, `ephemeral`) based on the Phase 1 inventory, without performing any destructive file operations.

## Constraints
*   **Environment:** Shared NFS (T440 + cesarops2). High risk of race conditions or lock contention if multiple agents write simultaneously.
*   **Safety:** Zero-deletion policy. The script must only write metadata; no `rm` or `mv` commands are permitted in this phase.
*   **Dependency Integrity:** Classification logic must not interfere with or break existing `fleet/`, `forge-health/`, or `nomad/` operational scripts.
*   **Data Integrity:** The output must strictly adhere to the schema defined by the Phase 1 `manifest.json`.

## Architecture Sketch
1.  **THINKER (Logic Layer):**
    *   Parses `var/script-inventory/latest/manifest.json`.
    *   Defines the heuristic logic for classification (e.g., if path contains `temp/` $\rightarrow$ `ephemeral`; if in `core/` $\rightarrow$ `keep`).
    *   Maps existing `keep/review/ephemeral` tiers to the new Forge-standardized verdict.
2.  **CODER (Implementation Layer):**
    *   Develops a Python-based classification engine.
    *   Implements a "Read-Only" scan of the NFS directory structure.
    *   Aggregates results into a single JSON object.
3.  **REVIEWER (Validation Layer):**
    *   Performs a checksum/count validation: `count(manifest.json entries) == count(forge-verdict.json entries)`.
    *   Verifies that no file paths in the verdict deviate from the actual NFS structure.

## Handoff to Coders
**Task 1: Manifest Parser**
*   Read `var/script-inventory/latest/manifest.json`.
*   Extract all file paths and current tier metadata.

**Task 2: Classification Engine**
*   Apply logic to assign `keep`, `archive`, or `ephemeral` to each path.
*   *Logic Rule:* If script is in `review/` tier $\rightarrow$ `archive`. If in `ephemeral/` tier $\rightarrow$ `ephemeral`. If in `keep/` tier $\rightarrow$ `keep`.

**Task 3: JSON Serializer**
*   Write `forge-verdict.json` to the root of the inventory directory.

**Acceptance Criteria for `forge-verdict.json`:**
*   **Schema:** `{"scripts": [{"path": "string", "verdict": "keep|archive|ephemeral", "reason": "string"}]}`.
*   **Completeness:** Must contain exactly 185 entries (or the total count found in Phase 1).
*   **Immutability:** The script must execute in `read-only` mode regarding the actual script files on NFS.

## Risks & Open Questions
*   **Risk: NFS Latency/Locking:** Large-scale directory traversal on shared NFS can cause IO wait. *Mitigation: Implement small sleep intervals between directory reads.*
*   **Risk: Schema Mismatch:** If the Phase 1 `manifest.json` contains paths that no longer exist due to manual intervention. *Mitigation: Implement a `path_exists` check during classification.*
*   **Open Question:** Should the `reason` field in `forge-verdict.json` include the specific Phase 1 tier that triggered the classification?
*   **Open Question:** Are there specific naming patterns (e.g., `*_test.sh`) that should override the directory-based classification?