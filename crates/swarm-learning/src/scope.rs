//! Learning scope definitions — where learning is enabled and how it's isolated.

use serde::{Deserialize, Serialize};

/// The scope at which a learning strategy operates.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum LearningScope {
    /// Learning applies to a single agent.
    Agent {
        /// The agent identifier.
        agent_id: String,
    },
    /// Learning applies to a team of agents.
    Team {
        /// The team identifier.
        team_id: String,
    },
    /// Learning applies to an entire tenant/organization.
    Tenant {
        /// The tenant identifier.
        tenant_id: String,
    },
    /// Learning applies to a specific workflow type.
    Workflow {
        /// The workflow identifier.
        workflow_id: String,
    },
    /// Global learning (framework-wide, use with extreme caution).
    #[default]
    Global,
}

impl LearningScope {
    /// Returns a label for logging.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Agent { .. } => "agent",
            Self::Team { .. } => "team",
            Self::Tenant { .. } => "tenant",
            Self::Workflow { .. } => "workflow",
            Self::Global => "global",
        }
    }
}

/// Configuration for enabling/disabling learning at various scopes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningScopeConfig {
    /// The scope this configuration applies to.
    pub scope: LearningScope,
    /// Whether learning is enabled at this scope.
    pub enabled: bool,
    /// Whether outputs require human approval at this scope.
    pub require_approval: bool,
    /// Maximum pending (unapproved) outputs before learning pauses.
    pub max_pending_outputs: Option<u64>,
}

impl LearningScopeConfig {
    /// Create a config that enables learning without approval requirements.
    pub fn enabled(scope: LearningScope) -> Self {
        Self {
            scope,
            enabled: true,
            require_approval: false,
            max_pending_outputs: None,
        }
    }

    /// Create a config that enables learning with mandatory approval.
    pub fn with_approval(scope: LearningScope) -> Self {
        Self {
            scope,
            enabled: true,
            require_approval: true,
            max_pending_outputs: Some(100),
        }
    }

    /// Create a config that disables learning.
    pub fn disabled(scope: LearningScope) -> Self {
        Self {
            scope,
            enabled: false,
            require_approval: false,
            max_pending_outputs: None,
        }
    }
}
