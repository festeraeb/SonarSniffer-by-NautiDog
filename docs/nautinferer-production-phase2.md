# NautiInferer production — Phase 2 (B5)

**Preset:** `golden-test-b5`  
**Forge test:** child missions + salvage settlement  
**Status:** specification for Forge B5 validation (implementation spans `nauti-inferer/` Phase 1 + mission orchestration)

## Goal

Extend the inference coordinator so a **parent mission** can spawn **child missions**, track them in a **commander chain**, and run **`settle_salvage`** when a branch completes (merge partial results, close open actions, emit audit logs).

Phase 1 (done): scheduler + node registry in `nauti-inferer/` — no HTTP inference yet.  
Phase 2 (this doc): mission graph + settlement hooks wired for production tests.

## Core identifiers

| Field | Meaning |
|-------|---------|
| `mission_id` | Runtime instance UUID for one running mission |
| `mission_template_id` | Reusable template (YAML/JSON manifest) |
| `parent_mission_id` | Parent when this row is a child |
| `commander_chain` | Ordered list of mission IDs from root → current |
| `action_id` | Single step inside a mission (tool call, inference job, salvage) |

## Child missions

When the director receives a template with `spawn_children: true`:

1. Load `mission_template_id` for each child spec.
2. Create `mission_id` per child; set `parent_mission_id` and append to `commander_chain`.
3. Emit `child_mission_log` event: `{ parent, child, template_id, step }`.
4. Children run on the scheduler queue (same node registry as Phase 1).
5. Parent blocks on `wait_children` or continues async per template `join_policy` (`all` | `any` | `none`).

## `settle_salvage`

Callable action when a mission branch ends (success, cancel, or partial failure):

```json
{
  "action": "settle_salvage",
  "mission_id": "<uuid>",
  "inputs": {
    "child_results": ["<mission_id>", "..."],
    "salvage_policy": "merge_logs | keep_best | discard_failed"
  }
}
```

**Behavior:**

- Merge child stdout/logs into parent `mission_log`.
- Persist salvage bundle under `research_log/missions/<mission_id>/salvage.json`.
- Mark child missions `settled` (no re-queue).
- If any child failed, parent gets `status: partial` unless `salvage_policy: discard_failed`.

## Commander chain

- Root mission: `commander_chain: [root_mission_id]`.
- Each spawn: `commander_chain.push(child_mission_id)`.
- Forge / MCP tools must log the chain on every `think_harder` query for B5 traceability.

## B5 acceptance checklist (Forge)

1. `think_harder` with query `NautiInferer Phase 2 child missions settle_salvage` returns nautivecs hits (not empty `[]`).
2. `read_file` on this doc succeeds.
3. Agent summarizes: child spawn, `settle_salvage`, `commander_chain` (plain English, no channel tags).
4. Optional: `cargo check -p nauti_inferer` if Rust stubs exist under `nauti-inferer/`.

## Reference paths

- Coordinator: `nauti-inferer/src/lib.rs`, `scheduler/`, `node/`
- Fleet preset: `cesarops-forge-v2/cluster_config.toml` → `golden-test-b5`
- nautivecs: `http://127.0.0.1:5003` (expect `chunks` > 0)
- WSO: `http://127.0.0.1:5010`

## Known gaps (2026-05-25)

- `settle_salvage` and child-mission director may still be spec-only; B5 validates orchestration + documentation path first.
- n8n `think_harder` workflow often returns truncated JSON; Forge should use inline nautivecs + WSO.
