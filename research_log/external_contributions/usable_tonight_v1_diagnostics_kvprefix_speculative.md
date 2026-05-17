# Usable-Tonight Bundle — Diagnostics + KV Prefix Cache + Speculative Verify

Source: cluster response to operator's compact ask, 2026-05-17.
Status: **REFERENCE — INTEGRATION-READY, drop-in patterns for all 3.
GPU-side stats buffer is the standout cleverness.**

## Verdict

Three integrations, all tonight-scope:

1. **Diagnostic system** — compile-time strip + runtime toggle + GPU-side
   stats buffer (the trick: never readback per-token, atomics on GPU
   produce summary-only). Removes 20-40% per claim.
2. **KV prefix cache** — token-hash trie with LRU eviction, max_tokens
   budget, prefix_match returns longest cached prefix. 5-50× perceived
   on chat. ~150 LOC.
3. **Speculative decoding verify** — corrects prior bug. Now does
   proper rejection sampling: `α = min(1, p_m/p_d)`, `accept iff u≤α`,
   first-rejection-stops-chain, repair-token sampling from
   `max(0, p_m-p_d)` normalized. KV rollback on rejection. 1.3-2.5×.

## Polish notes

### Ask 1 (diagnostics)
- `update_stats` uses `atomicMin/atomicMax` on f32 — **WGSL has no
  atomic float ops.** Bitcast to u32 with a sort-friendly transform
  (sign-flip-trick) then atomic on u32. ~10 extra lines. Otherwise
  shader fails to compile.
- The `#[cfg(feature = "engine-debug")]` gate + `if d.level !=
  Debug { return; }` runtime gate is double-protection. Keep both.
- `write_buffer` over `map_async` is correct — locked into lessons
  if not already.

### Ask 2 (KV prefix cache)
- `HashMap<TokenHash, KvNode>` collisions are statistically zero at
  64-bit hash, but **truncating prefixes by 1 token may leave KV
  state for position N pointing at a node that was evicted at depth
  N+5.** Need a "live frontier" pointer set across nodes.
- `lru: VecDeque<TokenHash>` — eviction by token hash but the trie
  is depth-N. LRU should track *node* not single token; use a
  generational arena instead of HashMap-of-HashMap to make eviction
  O(1) and reference-safe.
- `kv_cache.restore(kv)` — the actual KV cache buffer must support
  partial restore. Our existing `KvCache` is a flat append-only
  buffer; restoring requires resetting `pos` and reusing the
  pre-existing GPU-side bytes. Cheap, but the API doesn't exist yet.
- Eviction budget `max_tokens > 16384 * 4` = 65536 tokens. For our
  Qwen 1.5B GQA cache that's 65536 × 2 KV heads × 128 head_dim × 4
  bytes × 2 (K+V) = 134 MB cache index footprint at fp32, 67 MB at
  fp16. Reasonable on 16 GB P100. On 8 GB 1070 want to cap lower
  (16384 tokens = ~33 MB fp16).

### Ask 3 (speculative verify)
- `softmax(&main_logits[i])[t]` — naive softmax overflow trap once
  more. **4th strike**. Online max-subtract or full pass with stable
  exp. (Lessons already records this as universal cluster pattern.)
- `(p_m / (p_d + 1e-9))` — eps in denominator is the right move,
  but at typical fp32 precision `p_d` near 1e-30 gives garbage anyway.
  Clamp `p_d.max(1e-20)` is safer.
- `kv_cache.rollback_to(i)` — like Ask 2, the API doesn't exist yet
  on our `KvCache`. Implementation: store `pos` snapshot before
  speculative writes, on rejection reset `pos` and zero-fill the
  reverted slots (or trust they'll be overwritten — they will be).
- `sample_repair_token` over `max(0, p_m - p_d)` — math is right,
  but the implementation `diff_logits.iter().map(|x| x.max(0.0))`
  is operating on logits not probabilities. Need
  `softmax(p_m) - softmax(p_d)` first, then clip-to-zero, then
  re-normalize. Three steps, easy to get wrong.

## Integration sequence

Tonight order (lowest risk first):

1. **Diagnostic gate** (no GPU changes, ~30 LOC, env flag) — 1 hour
2. **Uniform pool** (small refactor, ~80 LOC) — 2 hours
3. **GPU-side stats buffer** (atomic-on-u32 fix + WGSL injection
   into existing kernels) — 2 hours
4. **KV prefix cache** (with arena rewrite) — 4 hours
5. **Speculative verify** (after multi-model lands; defer if needed) — 4 hours

Steps 1-3 buy 20-40% raw throughput. Step 4 buys 5-50× on chat
turns. Step 5 buys 1.3-2.5× but needs multi-model first.

If the operator can only land 2 tonight: Steps 1+4. Diagnostic gate
+ prompt cache = "engine feels usable" outcome.

## Verbatim source — preserved for integration reference

[Full content of operator's pasted cluster response captured in
chat history. Three sections: ASK 1 diagnostics+UBO pool, ASK 2
prefix cache (trie+LRU), ASK 3 speculative verify (rejection
sampling). Math derivation correct for ask 3, code skeletons need
the 4 polish fixes above before drop-in compile.]
