

```rust
pub struct SpeculativeDecoder {
    pub n_speculative: usize,
    pub accept_threshold: f32,
}

impl SpeculativeDecoder {
    pub fn new(n_speculative: usize, accept_threshold: f32) -> Self {
        Self {
            n_speculative,
            accept_threshold,
        }
    }

    /// Verifies a sequence of draft tokens against the verifier model's logits.
    /// 
    /// # Arguments
    /// * `draft_tokens` - The sequence of tokens proposed by the draft model.
    /// * `verifier_logits_per_pos` - A slice of logits for each position. 
    ///   The length must be at least `draft_tokens.len() + 1` to account for the 
    ///   token predicted at the final draft position.
    pub fn verify(
        &self,
        draft_tokens: &[u32],
        verifier_logits_per_pos: &[Vec<f32>],
    ) -> SpecResult {
        let mut accepted = Vec::new();
        let mut correction = None;
        let mut accept_count = 0;

        for i in 0..draft_tokens.len() {
            let logits = &verifier_logits_per_pos[i];
            if logits.is_empty() {
                break;
            }

            // Compute Softmax and find max prob/argmax
            let mut max_logit = f32::NEG_INFINITY;
            let mut argmax_idx = 0;
            for (idx, &val) in logits.iter().enumerate() {
                if val > max_logit {
                    max_logit = val;
                    argmax_idx = idx;
                }
            }

            // Numerical stability: subtract max logit
            let mut sum_exp = 0.0;
            let mut probs = vec![0.0; logits.len()];
            for (idx, &val) in logits.iter().enumerate() {
                let p = (val - max_logit).exp();
                probs[idx] = p;
                sum_exp += p;
            }

            // Normalize probabilities
            for p in probs.iter_mut() {
                *p /= sum_exp;
            }

            let draft_token_idx = draft_tokens[i] as usize;
            let draft_prob = if draft_token_idx < probs.len() {
                probs[draft_token_idx]
            } else {
                0.0
            };

            // The verifier's "confidence" is the probability of its most likely token
            let verifier_max_prob = probs[argmax_idx];

            // Check acceptance criteria
            // We use a threshold relative to the verifier's peak probability
            if draft_prob >= self.accept_threshold * verifier_max_prob && draft_prob > 0.0 {
                accepted.push(draft_tokens[i]);
                accept_count += 1;
            } else {
                // Divergence detected: provide the verifier's preferred token
                correction = Some(argmax_idx as u32);
                break;
            }
        }

        // If we exhausted all draft tokens without divergence, 
        // the next token is the one predicted at the last position.
        if accept_count == draft_tokens.len() && correction.is_none() {
            let last_logits = &verifier_logits_per_pos[draft_tokens.len()];
            let mut max_val = f32::NEG_INFINITY;
            let mut argmax = 0;
            for (idx, &val) in last_logits.iter().enumerate() {
                if val > max_val {
                    max_val = val;
                    argmax = idx;
                }
            }
            correction = Some(argmax as u32);
        }

        SpecResult {
            accepted,
            correction,
            accept_count,
        }
    }
}

pub struct SpecResult {
    pub accepted: Vec<u32>,
    pub correction: Option<u32>,
    pub accept_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_acceptance() {
        let decoder = SpeculativeDecoder::new(2, 0.9);
        let draft_tokens = vec![1, 2];
        // Logits for pos 0, pos 1, and the "next" pos 2
        let logits = vec![
            vec![0.1, 10.0, 0.1], // Pos 0: token 1 is very likely
            vec![0.1, 0.1, 10.0], // Pos 1: token 2 is very likely
            vec![10.0, 0.1, 0.1], // Pos 2: token 0 is very likely (the correction)
        ];

        let result = decoder.verify(&draft_tokens, &logits);
        assert_eq!(result.accept_count, 2);
        assert_eq!(result.accepted, vec![1, 2]);
        assert_eq!(result.correction, Some(0));
    }

    #[test]
    fn test_rejection_and_correction() {
        let decoder = SpeculativeDecoder::new(2, 0.9);
        let draft_tokens = vec![1, 2];
        // Pos 0: token 1 is likely
        // Pos 1: token 2 is UNLIKELY (token 5 is max)
        let logits = vec![
            vec![0.0, 10.0, 0.0, 0.0, 0.0, 0.0], 
            vec![0.0, 0.0, 0.0, 0.0, 0.0, 10.0], 
            vec![10.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ];

        let result = decoder.verify(&draft_tokens, &logits);
        assert_eq!(result.accept_count, 1);
        assert_eq!(result.accepted, vec![1]);
        assert_eq!(result.correction, Some(5)); // Should correct to token 5
    }
}
```
