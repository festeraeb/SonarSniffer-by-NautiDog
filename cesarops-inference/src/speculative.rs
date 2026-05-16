//! Speculative decoding: verify-and-accept algorithm.
//!
//! Drafted by Gemma-4-26B-MoE on P100 #0 (round 2 fleet dispatch), polished
//! to drop markdown fences and align with engine integration. The algorithm
//! is correct as-drafted; only structural cleanup was needed.
//!
//! See `dispatch_results/r2_gemma_speculative.json` for the raw output and
//! `nautivecs` lesson "speculative-decoding-pattern" for the orchestrator
//! note about the pattern.

pub struct SpeculativeDecoder {
    pub n_speculative: usize,
    pub accept_threshold: f32,
}

pub struct SpecResult {
    /// Drafted tokens that the verifier accepted.
    pub accepted: Vec<u32>,
    /// Verifier's preferred token at the rejection point, or — if every
    /// draft token was accepted — the verifier-predicted next token.
    pub correction: Option<u32>,
    /// Number of accepted drafts. Always equals `accepted.len()`.
    pub accept_count: usize,
}

impl SpeculativeDecoder {
    pub fn new(n_speculative: usize, accept_threshold: f32) -> Self {
        Self { n_speculative, accept_threshold }
    }

    /// Verify a sequence of draft tokens against verifier-model logits.
    ///
    /// `verifier_logits_per_pos` must have length `draft_tokens.len() + 1` —
    /// the final entry is the position past the last draft token, used to
    /// produce the next token when every draft is accepted.
    pub fn verify(
        &self,
        draft_tokens: &[u32],
        verifier_logits_per_pos: &[Vec<f32>],
    ) -> SpecResult {
        let mut accepted = Vec::new();
        let mut correction = None;
        let mut accept_count = 0usize;

        for i in 0..draft_tokens.len() {
            let logits = match verifier_logits_per_pos.get(i) {
                Some(l) if !l.is_empty() => l,
                _ => break,
            };

            // Stable softmax: subtract max, exp, normalize.
            let (argmax_idx, max_logit) = logits
                .iter()
                .enumerate()
                .fold((0usize, f32::NEG_INFINITY), |(bi, bv), (i, &v)| {
                    if v > bv { (i, v) } else { (bi, bv) }
                });
            let mut probs = Vec::with_capacity(logits.len());
            let mut sum_exp = 0.0f32;
            for &v in logits {
                let p = (v - max_logit).exp();
                probs.push(p);
                sum_exp += p;
            }
            for p in probs.iter_mut() {
                *p /= sum_exp;
            }

            let draft_idx = draft_tokens[i] as usize;
            let draft_prob = probs.get(draft_idx).copied().unwrap_or(0.0);
            let verifier_max_prob = probs[argmax_idx];

            // Accept when the verifier gives the drafted token at least
            // `accept_threshold` × peak probability. Threshold near 1.0 is
            // strict (rejects most drafts); near 0.0 is permissive.
            if draft_prob >= self.accept_threshold * verifier_max_prob && draft_prob > 0.0 {
                accepted.push(draft_tokens[i]);
                accept_count += 1;
            } else {
                correction = Some(argmax_idx as u32);
                break;
            }
        }

        // All drafts accepted: emit the verifier's pick at the trailing position.
        if accept_count == draft_tokens.len() && correction.is_none() {
            if let Some(last_logits) = verifier_logits_per_pos.get(draft_tokens.len()) {
                if !last_logits.is_empty() {
                    let (argmax, _) = last_logits.iter().enumerate().fold(
                        (0usize, f32::NEG_INFINITY),
                        |(bi, bv), (i, &v)| if v > bv { (i, v) } else { (bi, bv) },
                    );
                    correction = Some(argmax as u32);
                }
            }
        }

        SpecResult { accepted, correction, accept_count }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_acceptance() {
        let decoder = SpeculativeDecoder::new(2, 0.9);
        let draft = vec![1, 2];
        let logits = vec![
            vec![0.1, 10.0, 0.1],
            vec![0.1, 0.1, 10.0],
            vec![10.0, 0.1, 0.1],
        ];
        let r = decoder.verify(&draft, &logits);
        assert_eq!(r.accept_count, 2);
        assert_eq!(r.accepted, vec![1, 2]);
        assert_eq!(r.correction, Some(0));
    }

    #[test]
    fn rejection_at_second_token() {
        let decoder = SpeculativeDecoder::new(2, 0.9);
        let draft = vec![1, 2];
        let logits = vec![
            vec![0.0, 10.0, 0.0, 0.0, 0.0, 0.0],
            vec![0.0, 0.0, 0.0, 0.0, 0.0, 10.0],
            vec![10.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ];
        let r = decoder.verify(&draft, &logits);
        assert_eq!(r.accept_count, 1);
        assert_eq!(r.accepted, vec![1]);
        assert_eq!(r.correction, Some(5));
    }

    #[test]
    fn empty_draft() {
        // Empty draft = "no speculation, just give me the next-position pick".
        // accept_count is 0 and accepted is empty, but correction emerges from
        // the all-accepted branch (since draft.len() == accept_count == 0).
        let decoder = SpeculativeDecoder::new(0, 0.9);
        let logits = vec![vec![1.0, 5.0]]; // argmax = 1
        let r = decoder.verify(&[], &logits);
        assert_eq!(r.accept_count, 0);
        assert!(r.accepted.is_empty());
        assert_eq!(r.correction, Some(1));
    }
}
