//! Learning outputs — the results produced by learning strategies.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::scope::LearningScope;

fn default_learning_scope() -> LearningScope {
    LearningScope::Global
}

/// A unique identifier for a learning output / learning rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LearningRuleId(Uuid);

impl LearningRuleId {
    /// Create a new, random identifier.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create from a known UUID.
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Return the underlying UUID.
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for LearningRuleId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for LearningRuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for LearningRuleId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// The category of a learning output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LearningCategory {
    /// Preference ranking update.
    PreferenceAdaptation,
    /// Extracted pattern or workflow template.
    PatternExtraction,
    /// Feedback-based quality adjustment.
    FeedbackIncorporation,
    /// Reusable plan template.
    PlanTemplate,
    /// Heuristic score update.
    ScoringImprovement,
    /// Organization-specific knowledge fact.
    KnowledgeAccumulation,
    /// Suggested configuration change (requires approval).
    ConfigurationEvolution,
    /// Training data export (requires approval).
    FineTuningData,
    /// Custom category.
    Custom(String),
}

impl LearningCategory {
    /// Returns a stable label for CLI output and metrics.
    pub fn label(&self) -> &str {
        match self {
            Self::PreferenceAdaptation => "preference_adaptation",
            Self::PatternExtraction => "pattern_extraction",
            Self::FeedbackIncorporation => "feedback_incorporation",
            Self::PlanTemplate => "plan_template",
            Self::ScoringImprovement => "scoring_improvement",
            Self::KnowledgeAccumulation => "knowledge_accumulation",
            Self::ConfigurationEvolution => "configuration_evolution",
            Self::FineTuningData => "fine_tuning_data",
            Self::Custom(value) => value.as_str(),
        }
    }
}

impl std::str::FromStr for LearningCategory {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let normalized = value.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "preference_adaptation" => Ok(Self::PreferenceAdaptation),
            "pattern_extraction" => Ok(Self::PatternExtraction),
            "feedback_incorporation" => Ok(Self::FeedbackIncorporation),
            "plan_template" => Ok(Self::PlanTemplate),
            "scoring_improvement" => Ok(Self::ScoringImprovement),
            "knowledge_accumulation" => Ok(Self::KnowledgeAccumulation),
            "configuration_evolution" => Ok(Self::ConfigurationEvolution),
            "fine_tuning_data" => Ok(Self::FineTuningData),
            "" => Err("learning category cannot be empty".into()),
            _ => Ok(Self::Custom(normalized)),
        }
    }
}

/// A single learning output produced by a strategy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningOutput {
    /// Unique identifier.
    pub id: LearningRuleId,
    /// The learning category.
    pub category: LearningCategory,
    /// Human-readable description of what was learned.
    pub description: String,
    /// The primary learning scope this output belongs to.
    #[serde(default = "default_learning_scope")]
    pub scope: LearningScope,
    /// The agent that this learning applies to (if agent-scoped).
    pub agent_id: Option<String>,
    /// The tenant this learning belongs to.
    pub tenant_id: Option<String>,
    /// The full context used to derive this output (for review).
    pub context: serde_json::Value,
    /// The learning delta (what changed or should change).
    pub delta: serde_json::Value,
    /// Whether human approval is required before this is applied.
    pub requires_approval: bool,
    /// Current status of this learning output.
    pub status: LearningStatus,
    /// When this output was produced.
    pub created_at: DateTime<Utc>,
    /// When this output was applied (if applicable).
    pub applied_at: Option<DateTime<Utc>>,
}

impl LearningOutput {
    /// Create a new auto-applicable output (no approval required).
    pub fn auto(
        category: LearningCategory,
        description: impl Into<String>,
        delta: serde_json::Value,
    ) -> Self {
        Self {
            id: LearningRuleId::new(),
            category,
            description: description.into(),
            scope: LearningScope::Global,
            agent_id: None,
            tenant_id: None,
            context: serde_json::Value::Null,
            delta,
            requires_approval: false,
            status: LearningStatus::Pending,
            created_at: Utc::now(),
            applied_at: None,
        }
    }

    /// Create a new output that requires human approval.
    pub fn requires_review(
        category: LearningCategory,
        description: impl Into<String>,
        delta: serde_json::Value,
        context: serde_json::Value,
    ) -> Self {
        Self {
            id: LearningRuleId::new(),
            category,
            description: description.into(),
            scope: LearningScope::Global,
            agent_id: None,
            tenant_id: None,
            context,
            delta,
            requires_approval: true,
            status: LearningStatus::PendingApproval,
            created_at: Utc::now(),
            applied_at: None,
        }
    }

    /// Set the primary scope for this output.
    pub fn set_scope(&mut self, scope: LearningScope) {
        self.scope = scope;
    }

    /// Return a stable human-readable scope label.
    pub fn scope_label(&self) -> String {
        match &self.scope {
            LearningScope::Agent { agent_id } => format!("agent:{agent_id}"),
            LearningScope::Team { team_id } => format!("team:{team_id}"),
            LearningScope::Tenant { tenant_id } => format!("tenant:{tenant_id}"),
            LearningScope::Workflow { workflow_id } => format!("workflow:{workflow_id}"),
            LearningScope::Global => "global".into(),
        }
    }
}

/// The lifecycle status of a learning output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LearningStatus {
    /// Output has been produced but not yet processed.
    Pending,
    /// Output is waiting for human approval.
    PendingApproval,
    /// Output has been approved and applied.
    Applied,
    /// Output was rejected by a reviewer.
    Rejected,
    /// Output was applied but later rolled back.
    RolledBack,
}

impl LearningStatus {
    /// Returns `true` if this is a terminal status.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Rejected | Self::RolledBack)
    }

    /// Returns a short label for metrics.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::PendingApproval => "pending_approval",
            Self::Applied => "applied",
            Self::Rejected => "rejected",
            Self::RolledBack => "rolled_back",
        }
    }
}

/// The result of applying a learning output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningResult {
    /// The learning output that was applied.
    pub output_id: LearningRuleId,
    /// Whether the application succeeded.
    pub success: bool,
    /// A message describing the result.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_output_creation() {
        let output = LearningOutput::auto(
            LearningCategory::PreferenceAdaptation,
            "Prefer provider X for embeddings",
            serde_json::json!({"preferred_provider": "openai"}),
        );
        assert!(!output.requires_approval);
        assert_eq!(output.status, LearningStatus::Pending);
    }

    #[test]
    fn review_output_creation() {
        let output = LearningOutput::requires_review(
            LearningCategory::ConfigurationEvolution,
            "Increase timeout for analytics tasks",
            serde_json::json!({"timeout_secs": 600}),
            serde_json::json!({"reason": "repeated timeouts observed"}),
        );
        assert!(output.requires_approval);
        assert_eq!(output.status, LearningStatus::PendingApproval);
    }

    #[test]
    fn scope_label_uses_primary_scope() {
        let mut output = LearningOutput::auto(
            LearningCategory::PatternExtraction,
            "Capture workflow pattern",
            serde_json::json!({"ok": true}),
        );
        output.set_scope(LearningScope::Workflow {
            workflow_id: "intake".into(),
        });

        assert_eq!(output.scope_label(), "workflow:intake");
    }

    #[test]
    fn category_from_str_accepts_built_in_label() {
        let category = "plan_template".parse::<LearningCategory>().unwrap();
        assert_eq!(category, LearningCategory::PlanTemplate);
    }

    #[test]
    fn category_from_str_preserves_custom_values() {
        let category = "my_custom_category".parse::<LearningCategory>().unwrap();
        assert_eq!(category, LearningCategory::Custom("my_custom_category".into()));
    }
}
