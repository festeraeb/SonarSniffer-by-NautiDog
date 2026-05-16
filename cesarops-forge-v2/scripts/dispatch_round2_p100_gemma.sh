#!/bin/bash
# Dispatch to Gemma-4-26B-MoE on P100 #0 (port 5001)
# Task: Speculative decoding draft pipeline (Rust module)
set -e
OUT_DIR="/home/cesarops/wreckhunter2000-1/cesarops-forge-v2/dispatch_results"
mkdir -p "$OUT_DIR"

PROMPT='Write a complete Rust module `speculative.rs` for the cesarops-inference engine that implements simple draft+verify speculative decoding.

API:
```
pub struct SpeculativeDecoder {
    n_speculative: usize,        // tokens to draft per round (e.g. 4)
    accept_threshold: f32,       // verifier prob ratio to accept (e.g. 0.9)
}

impl SpeculativeDecoder {
    pub fn new(n_speculative: usize, accept_threshold: f32) -> Self;

    /// Given the verifier model logits for positions [draft_start..draft_start+n+1]
    /// and the draft tokens, return how many drafted tokens are accepted plus
    /// the verifier-corrected token at the rejection point.
    pub fn verify(
        &self,
        draft_tokens: &[u32],
        verifier_logits_per_pos: &[Vec<f32>], // outer len = n+1, inner = vocab
    ) -> SpecResult;
}

pub struct SpecResult {
    pub accepted: Vec<u32>,        // drafted tokens that passed
    pub correction: Option<u32>,   // verifier-chosen token where draft diverged
    pub accept_count: usize,
}
```

Algorithm: for each position i in 0..draft_tokens.len(): compute softmax of verifier_logits_per_pos[i], take prob of draft_tokens[i]. If draft prob >= accept_threshold * verifier_max_prob, accept and continue. Else stop, set correction = argmax(verifier_logits_per_pos[i]), break.

Include `use` statements, no external crates, add 2 unit tests with synthetic logits. Return ONLY the .rs file content, no markdown fences, no commentary.'

ESC_PROMPT=$(printf '%s' "$PROMPT" | python3 -c "import sys,json; print(json.dumps(sys.stdin.read()))")

curl -s -m 600 -X POST http://127.0.0.1:5001/api/v1/generate \
  -H "Content-Type: application/json" \
  -d "{\"prompt\": $ESC_PROMPT, \"max_length\": 2048, \"temperature\": 0.2, \"top_p\": 0.9, \"rep_pen\": 1.05}" \
  > "$OUT_DIR/r2_gemma_speculative.json" 2>&1

python3 -c "import json; d=json.load(open('$OUT_DIR/r2_gemma_speculative.json')); print(d.get('results',[{}])[0].get('text',''))" \
  > "$OUT_DIR/r2_gemma_speculative.rs" 2>"$OUT_DIR/r2_gemma_speculative.err"
echo "[Gemma-4] speculative.rs: $(wc -l < "$OUT_DIR/r2_gemma_speculative.rs") lines"
