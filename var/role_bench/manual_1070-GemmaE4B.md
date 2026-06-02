<!-- meta: {"label": "1070-GemmaE4B", "endpoint": "http://127.0.0.1:5202", "model": "gemma-4-E4B-it-Q4_K_M.gguf", "gpu": "GTX 1070", "words": 578, "elapsed_s": 28.5, "usage": {"completion_tokens": 980, "prompt_tokens": 271, "total_tokens": 1251, "prompt_tokens_details": {"cached_tokens": 0}}} -->

## Goal

The primary goal of Phase 2 is to process the inventory of $\sim 185$ shell scripts located in `var/script-inventory/latest/` and generate a definitive classification file, `forge-verdict.json`. This file must assign each script to one of three states: `keep`, `archive`, or `ephemeral`, based on predefined criteria, without performing any actual file deletions or modifications to the source scripts or the NFS structure.

## Constraints

1.  **Safety First:** Absolutely no destructive operations (e.g., `rm`, `mv` outside of designated staging/review areas) are permitted on the scripts residing in `var/script-inventory/latest/`.
2.  **System Integrity:** The solution must not interfere with the operational health of the fleet, Forge services, or Nomad deployments. Scripts critical to these systems must be flagged for review, not deletion.
3.  **Input Data:** The process must consume the manifest data from `var/script-inventory/latest/manifest.json` to identify the target scripts.
4.  **Output Format:** The final output must be a single, structured JSON file: `forge-verdict.json`.

## Architecture Sketch (Thinker $\rightarrow$ Coder $\rightarrow$ Reviewer)

1.  **Thinker (This Agent):** Defines the precise logic, constraints, and required output schema. It specifies the classification rules (e.g., "If script contains 'nomad' AND is in `keep` tier, classify as `keep`").
2.  **Coder (Implementer):** Develops the core processing engine (e.g., a Python service) that reads `manifest.json`, iterates over the scripts, applies the classification logic, and constructs the `forge-verdict.json` structure.
3.  **Reviewer (QA/Security Agent):** Validates the generated `forge-verdict.json` against the defined rules. It performs spot checks on high-risk scripts (e.g., those tagged as `keep` in the inventory but flagged as `ephemeral` by the logic) to ensure the classification is sound before deployment.

## Handoff to Coders

Implementers, please focus on building the processing engine based on the following tasks.

**Task 1: Inventory Ingestion**
*   **Action:** Develop a module to securely read and parse `var/script-inventory/latest/manifest.json`.
*   **Acceptance Criteria:** The module must successfully load the list of all $\sim 185$ script paths and their associated metadata (e.g., current tier: `keep`, `review`, `ephemeral`).

**Task 2: Classification Engine Implementation**
*   **Action:** Implement the core logic that iterates through each script path identified in Task 1. For each script, apply the classification rules (detailed in the design spec, which you will receive separately, but assume rules exist for `keep`, `archive`, `ephemeral`).
*   **Acceptance Criteria:** The engine must successfully classify every script path. A robust logging mechanism must capture *why* a script received a specific verdict (e.g., "Verdict: archive; Reason: Contains deprecated function X").

**Task 3: Verdict Generation**
*   **Action:** Construct the final `forge-verdict.json` file.
*   **Acceptance Criteria:**
    *   The file must contain a top-level mapping where keys are the full script paths (relative to the inventory root).
    *   The value for each key must be an object containing at least two fields: `"verdict": "keep" | "archive" | "ephemeral"` and `"reason": "string"`.
    *   The file must be written to a designated staging area (e.g., `var/script-inventory/staging/forge-verdict.json`).

## Risks & Open Questions

1.  **Classification Ambiguity:** The most significant risk is the ambiguity in the classification rules. If a script meets criteria for both `keep` and `archive`, the current design does not specify precedence. **(Action Required: Define precedence rules for overlapping criteria.)**
2.  **External Dependencies:** If the classification logic requires external system calls (e.g., checking Nomad status via API), these calls must be mocked during initial development to ensure the core logic is testable without impacting production infrastructure.
3.  **Manifest Schema Drift:** If `manifest.json` changes its structure between the time the inventory is taken and when Phase 2 runs, the ingestion module may fail. We must implement defensive parsing.