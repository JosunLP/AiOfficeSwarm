//! Personality boundary constraints.
//!
//! A [`PersonalityBoundary`] defines compliance-level restrictions on which
//! personality traits are permitted. The policy engine uses boundaries to
//! ensure personalities never bypass security or compliance rules.

use serde::{Deserialize, Serialize};

use crate::profile::RiskTolerance;

/// Constraints that limit what a personality profile may contain.
///
/// These are evaluated by the policy layer whenever a personality is applied
/// or overridden. A boundary is typically set per tenant or compliance domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalityBoundary {
    /// Maximum allowed risk tolerance. Profiles exceeding this are rejected.
    pub max_risk_tolerance: Option<RiskTolerance>,
    /// Allowed communication tones (empty = all allowed).
    pub allowed_tones: Vec<String>,
    /// Forbidden custom traits (by key name).
    pub forbidden_custom_traits: Vec<String>,
    /// Whether task-level personality overrides are permitted.
    pub allow_task_overlays: bool,
    /// Whether custom personalities may be loaded from plugins.
    pub allow_plugin_personalities: bool,
    /// Maximum verbosity level permitted.
    pub max_verbosity: Option<u8>,
}

impl Default for PersonalityBoundary {
    fn default() -> Self {
        Self {
            max_risk_tolerance: None,
            allowed_tones: Vec::new(),
            forbidden_custom_traits: Vec::new(),
            allow_task_overlays: true,
            allow_plugin_personalities: true,
            max_verbosity: None,
        }
    }
}

impl PersonalityBoundary {
    /// Check whether a risk tolerance level is within bounds.
    pub fn risk_within_bounds(&self, risk: RiskTolerance) -> bool {
        match self.max_risk_tolerance {
            None => true,
            Some(max) => (risk as u8) <= (max as u8),
        }
    }

    /// Check whether a tone is within bounds. If `allowed_tones` is empty,
    /// all tones are permitted.
    pub fn tone_within_bounds(&self, tone: &str) -> bool {
        if self.allowed_tones.is_empty() {
            return true;
        }
        self.allowed_tones.iter().any(|t| t == tone)
    }

    /// Check whether a verbosity level is within bounds.
    pub fn verbosity_within_bounds(&self, verbosity: u8) -> bool {
        match self.max_verbosity {
            None => true,
            Some(max) => verbosity <= max,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_boundary_allows_all() {
        let boundary = PersonalityBoundary::default();
        assert!(boundary.risk_within_bounds(RiskTolerance::VeryHigh));
        assert!(boundary.tone_within_bounds("casual"));
        assert!(boundary.verbosity_within_bounds(5));
    }

    #[test]
    fn boundary_restricts_risk() {
        let boundary = PersonalityBoundary {
            max_risk_tolerance: Some(RiskTolerance::Low),
            ..Default::default()
        };
        assert!(boundary.risk_within_bounds(RiskTolerance::Low));
        assert!(boundary.risk_within_bounds(RiskTolerance::VeryLow));
        assert!(!boundary.risk_within_bounds(RiskTolerance::Medium));
    }

    #[test]
    fn boundary_restricts_tone() {
        let boundary = PersonalityBoundary {
            allowed_tones: vec!["formal".into(), "professional".into()],
            ..Default::default()
        };
        assert!(boundary.tone_within_bounds("formal"));
        assert!(!boundary.tone_within_bounds("casual"));
    }
}
