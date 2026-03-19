//! Personality profile definition.
//!
//! A [`PersonalityProfile`] captures all the traits that influence how an
//! agent communicates, decides, and collaborates. Profiles are composable:
//! a task-specific overlay merges on top of a role-level profile, which
//! merges on top of an organization default.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::traits::PersonalityId;

/// Communication style preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommunicationStyle {
    /// Tone (e.g., `"formal"`, `"friendly"`, `"concise"`, `"technical"`).
    pub tone: String,
    /// Verbosity level: 1 (minimal) to 5 (very detailed).
    pub verbosity: u8,
    /// Preferred language for responses (BCP 47 tag, e.g., `"en-US"`).
    pub language: String,
    /// Whether the agent should use domain-specific jargon.
    pub use_jargon: bool,
}

impl Default for CommunicationStyle {
    fn default() -> Self {
        Self {
            tone: "professional".into(),
            verbosity: 3,
            language: "en-US".into(),
            use_jargon: false,
        }
    }
}

/// Decision-making tendencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionTendencies {
    /// Preference for speed vs. thoroughness (0.0 = fastest, 1.0 = most thorough).
    pub thoroughness: f64,
    /// Whether the agent prefers consensus before acting.
    pub consensus_seeking: bool,
    /// How much the agent values precedent vs. innovation (0.0 = conservative, 1.0 = innovative).
    pub innovation_bias: f64,
}

impl Default for DecisionTendencies {
    fn default() -> Self {
        Self {
            thoroughness: 0.5,
            consensus_seeking: false,
            innovation_bias: 0.3,
        }
    }
}

/// Risk tolerance level.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskTolerance {
    /// Avoid all risk; escalate if uncertain.
    VeryLow,
    /// Slight risk is acceptable with justification.
    Low,
    /// Balanced risk assessment.
    #[default]
    Medium,
    /// Comfortable with moderate risk.
    High,
    /// Willing to take significant risks.
    VeryHigh,
}

/// Collaboration pattern with other agents.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CollaborationPattern {
    /// Works independently; minimal coordination.
    Independent,
    /// Coordinates on demand when explicitly asked.
    #[default]
    Cooperative,
    /// Actively seeks collaboration and delegates.
    Collaborative,
    /// Leads and directs other agents.
    Directive,
}

/// Escalation behavior when encountering uncertainty or failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationBehavior {
    /// When to escalate: `"immediate"`, `"after_retry"`, `"never"`.
    pub trigger: String,
    /// Maximum retries before escalation.
    pub max_retries_before_escalation: u32,
    /// Whether to include full context in escalation reports.
    pub include_full_context: bool,
}

impl Default for EscalationBehavior {
    fn default() -> Self {
        Self {
            trigger: "after_retry".into(),
            max_retries_before_escalation: 2,
            include_full_context: true,
        }
    }
}

/// Response formatting preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseFormatting {
    /// Preferred output format (e.g., `"markdown"`, `"plain"`, `"json"`).
    pub format: String,
    /// Whether to include reasoning/thinking steps in output.
    pub show_reasoning: bool,
    /// Whether to include confidence indicators.
    pub show_confidence: bool,
    /// Maximum response length hint (in characters; 0 = no limit).
    pub max_length_hint: usize,
}

impl Default for ResponseFormatting {
    fn default() -> Self {
        Self {
            format: "markdown".into(),
            show_reasoning: false,
            show_confidence: false,
            max_length_hint: 0,
        }
    }
}

/// A complete personality profile for an agent.
///
/// Personalities are composable: overlays merge on top of base profiles.
/// `None` fields in an overlay mean "inherit from the layer below."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalityProfile {
    /// Unique identifier for this personality.
    pub id: PersonalityId,
    /// Human-readable name (e.g., `"Enterprise Analyst"`).
    pub name: String,
    /// Semantic version of this personality definition.
    pub version: String,
    /// Optional description.
    pub description: Option<String>,
    /// Communication style preferences.
    pub communication_style: CommunicationStyle,
    /// Decision-making tendencies.
    pub decision_tendencies: DecisionTendencies,
    /// Risk tolerance.
    pub risk_tolerance: RiskTolerance,
    /// Collaboration pattern.
    pub collaboration_pattern: CollaborationPattern,
    /// Escalation behavior.
    pub escalation_behavior: EscalationBehavior,
    /// Domain specialization hints (e.g., `["finance", "compliance"]`).
    pub domain_hints: Vec<String>,
    /// Response formatting preferences.
    pub response_formatting: ResponseFormatting,
    /// Arbitrary custom traits for extension.
    pub custom_traits: HashMap<String, serde_json::Value>,
}

impl PersonalityProfile {
    /// Create a new personality profile with defaults.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            id: PersonalityId::new(),
            name: name.into(),
            version: version.into(),
            description: None,
            communication_style: CommunicationStyle::default(),
            decision_tendencies: DecisionTendencies::default(),
            risk_tolerance: RiskTolerance::default(),
            collaboration_pattern: CollaborationPattern::default(),
            escalation_behavior: EscalationBehavior::default(),
            domain_hints: Vec::new(),
            response_formatting: ResponseFormatting::default(),
            custom_traits: HashMap::new(),
        }
    }

    /// Merge an overlay on top of this profile.
    ///
    /// The overlay's non-default fields take precedence. This is used to
    /// apply task-specific or role-specific overrides.
    pub fn merge_overlay(&self, overlay: &PersonalityOverlay) -> PersonalityProfile {
        let mut merged = self.clone();
        if let Some(ref style) = overlay.communication_style {
            merged.communication_style = style.clone();
        }
        if let Some(ref tendencies) = overlay.decision_tendencies {
            merged.decision_tendencies = tendencies.clone();
        }
        if let Some(risk) = overlay.risk_tolerance {
            merged.risk_tolerance = risk;
        }
        if let Some(collab) = overlay.collaboration_pattern {
            merged.collaboration_pattern = collab;
        }
        if let Some(ref esc) = overlay.escalation_behavior {
            merged.escalation_behavior = esc.clone();
        }
        if let Some(ref hints) = overlay.domain_hints {
            merged.domain_hints = hints.clone();
        }
        if let Some(ref fmt) = overlay.response_formatting {
            merged.response_formatting = fmt.clone();
        }
        for (k, v) in &overlay.custom_traits {
            merged.custom_traits.insert(k.clone(), v.clone());
        }
        merged
    }
}

/// A partial personality overlay that can be merged on top of a base profile.
///
/// All fields are optional — only set fields override the base.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersonalityOverlay {
    /// Override communication style.
    pub communication_style: Option<CommunicationStyle>,
    /// Override decision tendencies.
    pub decision_tendencies: Option<DecisionTendencies>,
    /// Override risk tolerance.
    pub risk_tolerance: Option<RiskTolerance>,
    /// Override collaboration pattern.
    pub collaboration_pattern: Option<CollaborationPattern>,
    /// Override escalation behavior.
    pub escalation_behavior: Option<EscalationBehavior>,
    /// Override domain hints.
    pub domain_hints: Option<Vec<String>>,
    /// Override response formatting.
    pub response_formatting: Option<ResponseFormatting>,
    /// Additional custom traits (merged additively).
    #[serde(default)]
    pub custom_traits: HashMap<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_creation() {
        let profile = PersonalityProfile::new("Test Agent", "1.0.0");
        assert_eq!(profile.name, "Test Agent");
        assert_eq!(profile.version, "1.0.0");
        assert_eq!(profile.risk_tolerance, RiskTolerance::Medium);
    }

    #[test]
    fn overlay_merge() {
        let base = PersonalityProfile::new("Base", "1.0.0");
        let overlay = PersonalityOverlay {
            risk_tolerance: Some(RiskTolerance::Low),
            domain_hints: Some(vec!["security".into()]),
            ..Default::default()
        };
        let merged = base.merge_overlay(&overlay);
        assert_eq!(merged.risk_tolerance, RiskTolerance::Low);
        assert_eq!(merged.domain_hints, vec!["security"]);
        // Unchanged fields inherit from base.
        assert_eq!(
            merged.communication_style.tone,
            base.communication_style.tone
        );
    }
}
