# Code Reorganization Spec

## YOUR TASK
Read every file in `backup/src/` and organize them into a clean deployment structure.

## OUTPUT STRUCTURE
Create these directories inside `backup/` and sort files into them:

```
backup/
├── src/                    ← RAW COPY (DO NOT MODIFY)
├── deploy/                 ← Clean working structure (you build this)
│   ├── inference/          ← LLM inference engine (cesarops-inference)
│   ├── agent/              ← Orchestration & agent harness (cesarops-agent)
│   ├── detection/          ← Wreck detection pipeline (cesarops-detection)
│   ├── forge/              ← Web UI & tool execution (cesarops-forge-v2)
│   ├── tools/              ← Specialized tools (satellite_stitch, optical_mass, etc.)
│   ├── scripts/            ← Shell scripts, service files, utilities
│   ├── config/             ← All config files (TOML, JSON, YAML)
│   ├── shaders/            ← WGSL compute shaders
│   ├── web/                ← HTML/CSS/JS frontend files
│   └── docs/               ← Documentation, specs, roadmaps
├── DELETE_CANDIDATES.md    ← Files that serve no purpose (you write this)
├── NEEDS_COMPLETION.md     ← Files that are stubs/incomplete but useful (you write this)
├── NEEDS_WRITING.md        ← Files that SHOULD exist but don't yet (you write this)
└── REORGANIZATION_SPEC.md  ← This file
```

## RULES
1. DO NOT modify anything in `backup/src/` — that's the raw backup
2. COPY files from `src/` into `deploy/` in the correct category
3. If a file belongs in multiple categories, put it in the primary one
4. For each file you process, note in your assessment:
   - What it does (1 sentence)
   - Whether it compiles/works or is a stub
   - What section it belongs to

## DELETE_CANDIDATES.md FORMAT
```markdown
# Files to Delete
| File | Reason |
|------|--------|
| path/to/file.rs | Empty stub, never called, duplicated by X |
```

## NEEDS_COMPLETION.md FORMAT
```markdown
# Files Needing Completion
| File | What's Done | What's Missing | Priority |
|------|-------------|----------------|----------|
| path/to/file.rs | Struct defined, 2 methods | Missing 3 methods, no tests | High |
```

## NEEDS_WRITING.md FORMAT
```markdown
# Files That Should Be Created
| Suggested Path | Purpose | Why It's Needed | Priority |
|---------------|---------|-----------------|----------|
| deploy/tools/curvelet_slicer.rs | FFT drift correction | Coordinate drift in satellite tiles | High |
```

## CONTEXT
This is the WreckHunter 2000 project — a maritime search & rescue system that uses:
- LLM inference (custom Rust engine with wgpu GPU dispatch)
- Satellite imagery analysis (curvelet transforms, spectral filtering)
- Aeromagnetic anomaly detection (magnetic dipole analysis)
- Multi-node GPU cluster orchestration (P100s, 1070, 1060, future M10s)
- Agent-based autonomous task execution

The goal is shipwreck detection in the Great Lakes (Michigan, Superior, Huron, Erie, Ontario).

## HOW TO WORK
1. Use `read_file` to examine each file in `backup/src/`
2. Use `run_command` with `ls` to see the directory structure
3. Use `write_file` to create the deploy structure and assessment docs
4. Work through files systematically — don't skip any
5. When done, use `run_command` with `tree backup/deploy/` to show the final layout
