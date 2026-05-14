# Telemetry & Patching for SonarSniffer / CESAROPS

This document describes the two-way telemetry and patching workflow.

## Goals
- Collect parse failure reports and telemetry when parsing sonar files.
- Allow operators to propose and approve small, safe patches (e.g., alternate magic header values) and push them to clients.
- Keep auto-apply conservative and auditable; never auto-apply arbitrary code without review.

## Components
- `src/sonarsniffer/telemetry.py` — client helpers:
  - `send_parse_report(...)`
  - `fetch_pending_patches(...)`
  - `apply_patch(...)` — safe auto-apply for `magic_variants` patches
  - `verify_patch_signature(...)` — HMAC-based signature verification
  - `report_patch_applied(...)` — reports result back to server
  - `submit_patch(...)` — submit a patch proposal to the server

- `scripts/scan_garmin_firmware.py` — discover candidate 4-byte magic header variants from firmware blobs.
- `scripts/poll_and_apply_patches.py` — poll pending patches and apply safe patches (`--auto-apply` required).
- `scripts/create_patch_pr.py` — helper to add a patch JSON to `patches/pending/` and create a branch/PR.
- `scripts/submit_patch.py` — submit a patch JSON to the telemetry server.
- `scripts/patch_webhook_receiver.py` — example webhook receiver that writes approved patches to `patches/approved/` (verifies HMAC header).

## Security Model
- Patches can be signed by the server with an HMAC-SHA256 using shared secret `SONARSNIFFER_PATCH_SECRET`.
- Clients verify signatures before auto-applying. If the secret is not set, unsigned patches are allowed only for non-code patch types and with manual review.
- Code patches are not auto-applied. `create_patch_pr.py` helps create a PR for manual review.

## Example workflow
1. Collect firmware and run `scan_garmin_firmware.py` to generate candidates.
2. Create a patch JSON (type `magic_variants`) and submit with `scripts/submit_patch.py` or create PR via `scripts/create_patch_pr.py`.
3. Server approves and signs patch; clients poll and apply when `--auto-apply` is set.
4. Clients report patch application back to server using `report_patch_applied`.

## CI & Benchmarks
- A GitHub Actions workflow `rust-wheels-and-benchmarks.yml` builds Rust wheels (maturin) and runs the benchmark harness `scripts/benchmark_parser.py`.

## Notes
- Keep `garmin_magic_variants.txt` under version control for traceability where possible.
- For production, prefer asymmetrically-signed patches (e.g., ed25519) and server-side attestations.
