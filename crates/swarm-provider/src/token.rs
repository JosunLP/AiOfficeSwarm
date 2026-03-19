//! Token usage and cost accounting.

use serde::{Deserialize, Serialize};

/// Token usage statistics from a model response.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Number of tokens in the prompt/input.
    pub prompt_tokens: u64,
    /// Number of tokens generated in the completion/output.
    pub completion_tokens: u64,
    /// Total tokens (prompt + completion).
    pub total_tokens: u64,
    /// Estimated cost in the smallest currency unit (e.g., microdollars).
    /// `None` if the provider does not report cost.
    pub cost_micros: Option<u64>,
}

impl TokenUsage {
    /// Create a usage record from prompt and completion token counts.
    pub fn new(prompt_tokens: u64, completion_tokens: u64) -> Self {
        Self {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
            cost_micros: None,
        }
    }

    /// Attach a cost estimate.
    pub fn with_cost(mut self, cost_micros: u64) -> Self {
        self.cost_micros = Some(cost_micros);
        self
    }

    /// Merge another usage record into this one (e.g., for multi-turn totals).
    pub fn merge(&mut self, other: &TokenUsage) {
        self.prompt_tokens += other.prompt_tokens;
        self.completion_tokens += other.completion_tokens;
        self.total_tokens += other.total_tokens;
        match (&mut self.cost_micros, other.cost_micros) {
            (Some(a), Some(b)) => *a += b,
            (None, Some(b)) => self.cost_micros = Some(b),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_usage_new() {
        let u = TokenUsage::new(100, 50);
        assert_eq!(u.total_tokens, 150);
        assert_eq!(u.cost_micros, None);
    }

    #[test]
    fn token_usage_merge() {
        let mut a = TokenUsage::new(100, 50).with_cost(1000);
        let b = TokenUsage::new(200, 100).with_cost(2000);
        a.merge(&b);
        assert_eq!(a.prompt_tokens, 300);
        assert_eq!(a.completion_tokens, 150);
        assert_eq!(a.total_tokens, 450);
        assert_eq!(a.cost_micros, Some(3000));
    }
}
