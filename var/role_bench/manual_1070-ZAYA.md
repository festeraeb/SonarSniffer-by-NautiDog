<!-- meta: {"endpoint": "http://10.0.0.201:5203", "finish_reason": "length", "usage": {"completion_tokens": 733, "prompt_tokens": 291, "total_tokens": 1024, "prompt_tokens_details": {"cached_tokens": 0}}, "elapsed_s": 128.5, "content_words": 16, "reasoning_words": 396} -->

<!-- final content -->
## Goal
Produce a Phase 2 deliverable for the migration project: **classify every shell script in

<!-- reasoning_content -->
We need to produce a structured outline with sections: Goal, Constraints, Architecture sketch, Handoff to coders (numbered tasks + acceptance criteria for forge-verdict.json), Risks & open questions. Must be under 700 words. Must not write code. Must be from THINKER perspective? Probably a structured outline with sections.

We have 185 shell scripts on shared NFS. Phase 2: classify each script into keep / archive / ephemeral and write forge-verdict.json. No deletes yet.

We have constraints: NFS shared T440+cesarops2, do not break fleet/forge-health/nomad scripts. So we must note constraints.

We need to produce sections: Goal, Constraints, Architecture sketch (how thinker → coder → reviewer would execute Phase 2), Handoff to coders (numbered tasks + acceptance criteria for forge-verdict.json), Risks & open questions.

We must be specific to our repo paths. We have var/script-inventory/latest/ manifest.json, report.md, keep/review/ephemeral tiers. Possibly repo paths: var/scripts/ or something. We can assume paths: var/script-inventory/latest/ manifest.json, var/scripts/ maybe. But we need to be specific: "our repo paths". Could be /opt/scripts/ or /var/scripts/. Let's assume the inventory is at var/script-inventory/latest/ manifest.json. We'll note that.

We need to keep under 700 words. Let's aim for maybe ~500 words.

We must not write code. So just outline.

We need to think: The THINKER is in a multi-agent pipeline: thinker, coder, reviewer. So maybe we outline tasks for each role.

We'll produce sections with headings.

We must include constraints: do not break fleet/forge-health/nomad scripts. Means we must avoid affecting those scripts.

We need to include Architecture sketch: how thinker → coder → reviewer would execute Phase 2. So maybe a high-level flow: Identify each script from inventory, assign classification, write JSON.

Handoff to coders: numbered tasks + acceptance criteria for forge-verdict.json. So list tasks for coders: retrieve each script, classify based on criteria, write verdict.

Risks & open questions: potential issues, unknown criteria, etc.

Let's write.

Word count: Let's keep each section concise.

We'll start with:

## Goal
... etc.

Make sure to include constraints: NFS shared T440+cesarops2, preserve fleet/forge-health/nomad scripts.

Architecture sketch: maybe two-step: Planner (Thinker) defines inventory extraction, classification rules; Coder implements script classification; Reviewer validates and commits verdict to forge-verdict.json.

Handoff: Numbered tasks: 1. Pull manifest; 2. For each script, fetch metadata; 3. Apply classification rules; 4. Generate forge-verdict.json; 5. Commit.

Acceptance criteria: each script assigned correct tier, JSON includes mapping, etc.

Risks: script dependencies, classification ambiguous, write performance, etc.

Ok.

Now produce final answer.
