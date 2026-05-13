# Session Scratchpad — Live Status

## What's Running Right Now
- [x] Forge-v2 on T440:9100 (three-brain corrector logic)
- [x] KoboldCPP 35B MoE on P100s (:5001)
- [x] R1-32B on Xeon CPU (:5557) — answering Q1 (risks)
- [x] nautivecs on :5003 (12,600+ chunks)
- [x] WSO on :5010

## Downloads Complete
- [x] DeepSeek-R1-Distill-Qwen-32B-Q4_K_M.gguf (19GB) — /codebase/models/
- [x] Fortytwo_Strand-Rust-Coder-14B-BF16.gguf (28GB) — /codebase/models/
- [x] nomic-embed-text-v1.5.Q8_0.gguf (140MB) — cesarops2

## Disk Space Fixed
- Root partition: was 0 bytes free, now 6.5GB (removed old repo + snaps)

## R1 Assessment Progress
- [ ] Q1: Risks — PENDING (R1 thinking...)
- [ ] Q2: Strengths — waiting
- [ ] Q3: Cutting edge techniques — waiting
- [ ] Q4: Rust crates/projects — waiting
- [ ] Q5: Performance expectations — waiting

## After R1 Assessment
1. Review answers with Captain + Gemini
2. Hot-swap: kill R1, load Strand BF16 on Xeon
3. Begin Burn/Cake build with Strand as corrector
4. 35B writes code, Strand audits, nautivecs learns

## Quick Reference
- T440 SSH: cesarops@100.72.182.77 (pw: cesarops)
- cesarops2: 100.102.158.111
- cesarops3: 100.105.77.74
- Forge UI: http://100.72.182.77:9100/
- R1 API: http://100.72.182.77:5557/api/v1/generate
