### P100-Gemma-MoE: UX/Script Inventory & CLI Spec

**File Path:** `inventory/UX_SPEC.md`

**Core Logic / Task:**
Perform an audit of the `wreckhunter2000-1` repository to map existing lifecycle scripts (launch, monitor, teardown). I will identify overlapping logic in legacy scripts and design a unified CLI wrapper that abstracts the new dynamic watchdog.

**Implementation Sketch (CLI Structure):**
```bash
# Proposed CLI Interface
wreckhunter [command] [target] --port [p]
  -> wreckhunter status          # Aggregates heartbeat snapshots
  -> wreckhunter reload [gpu]    # Triggers watchdog for specific UUID
  -> wreckhunter deploy [path]   # Updates heartbeat and triggers reload
```

**Acceptance Criteria:**
1.  **Inventory Table:** A complete mapping of current scripts vs. proposed unified commands.
2.  **Dependency Graph:** Visual/textual map of how the watchdog interacts with existing port-management scripts.
3.  **Deprecation List:** Identification of at least 3 legacy scripts to be replaced by the new UX.
4.  **Handoff:** Pass the UX spec to `P100-Qwen36` to ensure CLI commands align with the heartbeat schema.

**What I did NOT do:**
*   I did not write the actual shell scripts for the CLI.
*   I did not implement the watchdog logic itself.
*   I did not define the data schema.

---

**Handoff to P100-Qwen36:** *Please ensure the `UX_SPEC.md` command arguments (e.g., `--port`, `--uuid`) are strictly validated against your proposed JSON schema.*